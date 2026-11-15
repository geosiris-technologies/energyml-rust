//! Whether a station's azimuth means anything (a hole close to the vertical has an azimuth that is noise).

use energyml_geometry::min_curvature::Measured;

/// What a criterion sees of one station.
pub struct StationContext<'a> {
    /// The station, from 0, in `stations`.
    pub index: usize,
    /// Every station of the survey.
    pub stations: &'a [Measured],
}

/// A rule that decides whether a station's azimuth means something. A registry: [`Criteria`].
pub trait SignificanceCriterion: Send + Sync {
    /// A stable identifier, for a recipe to name its choice.
    fn id(&self) -> &'static str;
    /// `true` when the azimuth is significant there. Called only on stations that state an azimuth.
    fn significant(&self, at: &StationContext<'_>) -> bool;
}

/// Significant at or above this inclination, in degrees (the threshold included). Id `inclination-at-least`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InclinationAtLeast(pub f64);

impl SignificanceCriterion for InclinationAtLeast {
    fn id(&self) -> &'static str {
        "inclination-at-least"
    }

    fn significant(&self, at: &StationContext<'_>) -> bool {
        match at.stations.get(at.index).and_then(|s| s.inclination) {
            Some(inclination) => inclination.is_finite() && self.0.is_finite() && inclination >= self.0,
            None => false,
        }
    }
}

/// Significant when an azimuth error of `azimuth_error` degrees would move the hole sideways by at least
/// `length` metres over the station's course: `ΔMD × sin(I) × azimuth_error (rad) >= length`.
/// Id `lateral-effect-at-least`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LateralEffectAtLeast {
    /// The azimuth error assumed, in degrees.
    pub azimuth_error: f64,
    /// The sideways displacement from which the azimuth matters, in metres.
    pub length: f64,
}

impl SignificanceCriterion for LateralEffectAtLeast {
    fn id(&self) -> &'static str {
        "lateral-effect-at-least"
    }

    fn significant(&self, at: &StationContext<'_>) -> bool {
        let stations = at.stations;
        let Some(station) = stations.get(at.index) else {
            return false;
        };
        let Some(inclination) = station.inclination else {
            return false;
        };
        // The course that ends at the station; the first station takes the one that starts there.
        let course = if at.index > 0 {
            station.md - stations[at.index - 1].md
        } else if stations.len() > 1 {
            stations[1].md - station.md
        } else {
            return false;
        };
        let effect = course * inclination.to_radians().sin() * self.azimuth_error.to_radians();
        effect.is_finite() && self.length.is_finite() && effect >= self.length
    }
}

/// The criteria, the caller's first.
pub struct Criteria {
    items: Vec<Box<dyn SignificanceCriterion>>,
}

impl Criteria {
    /// The built-ins: `InclinationAtLeast(3.0)`, then `LateralEffectAtLeast { azimuth_error: 1.0, length: 0.1 }`.
    pub fn builtin() -> Criteria {
        Criteria {
            items: vec![
                Box::new(InclinationAtLeast(3.0)),
                Box::new(LateralEffectAtLeast {
                    azimuth_error: 1.0,
                    length: 0.1,
                }),
            ],
        }
    }

    /// Tried before every criterion already there.
    pub fn register(&mut self, criterion: Box<dyn SignificanceCriterion>) {
        self.items.insert(0, criterion);
    }

    /// The first criterion that carries this id.
    pub fn get(&self, id: &str) -> Option<&dyn SignificanceCriterion> {
        self.items.iter().find(|c| c.id() == id).map(|c| c.as_ref())
    }

    /// The first one: the default.
    pub fn first(&self) -> &dyn SignificanceCriterion {
        self.items
            .first()
            .map(|c| c.as_ref())
            .expect("the built-in criteria are never removed")
    }
}

/// What a station's azimuth is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Significance {
    /// The azimuth means something there.
    Significant,
    /// The azimuth is stated and means nothing there.
    NotSignificant,
    /// The station states no azimuth.
    NoAzimuth,
}

/// One answer per station. A station that states no azimuth is `NoAzimuth`, and the criterion is not asked.
pub fn azimuth_significance(
    stations: &[Measured],
    criterion: &dyn SignificanceCriterion,
) -> Vec<Significance> {
    stations
        .iter()
        .enumerate()
        .map(|(index, station)| {
            if station.azimuth.is_none() {
                Significance::NoAzimuth
            } else if criterion.significant(&StationContext { index, stations }) {
                Significance::Significant
            } else {
                Significance::NotSignificant
            }
        })
        .collect()
}
