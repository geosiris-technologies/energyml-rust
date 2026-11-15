//! What the file states at its stations against what the method computes from them: two sources of the same
//! fact, one of which is not code written here. Nothing is judged here: there is no threshold, the caller judges.

use energyml_geometry::around_well::projection::VerticalSection;
use energyml_geometry::min_curvature::{Course, Located};
use energyml_geometry::stations::{Kept, StatedTvd, SurveyStations, TvdReference, UnitFault, same_object};

use super::section::ChosenSection;

/// A value a survey states at a station and this module compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatedField {
    /// The true vertical depth.
    Tvd,
    /// The north displacement.
    DispNs,
    /// The east displacement.
    DispEw,
    /// The abscissa along the vertical section.
    VertSect,
    /// The dogleg severity.
    Dls,
}

/// One value compared: stated by the file, computed from the stations (metres, or radians per metre for `Dls`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatedGap {
    /// The station, from 1, in the file.
    pub station: usize,
    /// Which value.
    pub field: StatedField,
    /// What the file states.
    pub stated: f64,
    /// What the method computes.
    pub computed: f64,
}

/// Why a stated value was not compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotComparedWhy {
    /// The TVD counts from another reference point than the MD datum.
    TvdFromAnotherPoint,
    /// The TVD counts from a vertical CRS: comparing it needs the datum's elevation in that CRS.
    TvdFromAVerticalCrs,
    /// The unit cannot be read.
    Unit(UnitFault),
    /// `GridScaleFactorUsed` is stated: the displacements are scaled and the factor is not read.
    GridScaleFactor,
    /// No section azimuth was given for `VertSect`.
    NoSection,
    /// The first station closes no course: it has no DLS.
    FirstStation,
    /// The station was left out (rejected).
    LeftOut,
}

/// A stated value left out of the comparison, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct NotCompared {
    /// The station, from 1, in the file.
    pub station: usize,
    /// Which value.
    pub field: StatedField,
    /// Why it was not compared.
    pub why: NotComparedWhy,
}

/// Every comparison made, and every one that was not.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatedReport {
    /// The values compared.
    pub gaps: Vec<StatedGap>,
    /// The values not compared.
    pub not_compared: Vec<NotCompared>,
}

/// What the comparison refuses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StatedError {
    /// The positions and courses do not match the kept stations.
    #[error("{located} position(s) and {courses} course(s) for {kept} kept station(s)")]
    LengthsDiffer {
        /// How many stations were kept.
        kept: usize,
        /// How many positions were given.
        located: usize,
        /// How many courses were given.
        courses: usize,
    },
}

impl StatedReport {
    fn compared(&mut self, station: usize, field: StatedField, stated: f64, computed: f64) {
        self.gaps.push(StatedGap {
            station,
            field,
            stated,
            computed,
        });
    }

    fn left(&mut self, station: usize, field: StatedField, why: NotComparedWhy) {
        self.not_compared.push(NotCompared { station, field, why });
    }
}

/// Compares what `stations` states with `located` and `courses`, where `located = positions(&kept.measured)` and
/// `courses = courses(&kept.measured)` - the stations as the file states them, azimuths uncorrected: the file's
/// displacements are in its own north (WITSML TRG 2.1 section 16.10 p. 357).
///
/// There is no threshold here: every comparison is reported, the caller judges. `RateBuild` and `RateTurn` are
/// never compared and never listed.
pub fn stated_against_computed(
    stations: &SurveyStations,
    kept: &Kept,
    located: &[Located],
    courses: &[Course],
    section: Option<&ChosenSection>,
) -> Result<StatedReport, StatedError> {
    let n = kept.stations.len();
    if located.len() != n || courses.len() + 1 != n {
        return Err(StatedError::LengthsDiffer {
            kept: n,
            located: located.len(),
            courses: courses.len(),
        });
    }
    let mut report = StatedReport::default();
    for stated in &stations.stated {
        let s = stated.station;
        let present = [
            (StatedField::Tvd, stated.tvd.is_some()),
            (StatedField::DispNs, stated.disp_ns.is_some()),
            (StatedField::DispEw, stated.disp_ew.is_some()),
            (StatedField::VertSect, stated.vert_sect.is_some()),
            (StatedField::Dls, stated.dls.is_some()),
        ];
        // `kept.stations` runs in file order: a binary search finds the station's rank.
        let Some(j) = kept.stations.binary_search(&s).ok() else {
            for (field, here) in present {
                if here {
                    report.left(s, field, NotComparedWhy::LeftOut);
                }
            }
            continue;
        };
        let place = &located[j];

        match &stated.tvd {
            None => {}
            Some(Err(fault)) => report.left(s, StatedField::Tvd, NotComparedWhy::Unit(fault.clone())),
            Some(Ok(StatedTvd { metres, from })) => match from {
                TvdReference::VerticalCrs(_) => report.left(s, StatedField::Tvd, NotComparedWhy::TvdFromAVerticalCrs),
                TvdReference::Point(point) => {
                    let datum = stations
                        .datums
                        .per_station
                        .get(s - 1)
                        .and_then(|d| d.as_deref())
                        .or(stations.datums.default.as_deref());
                    if datum.is_some_and(|d| same_object(point, d)) {
                        report.compared(s, StatedField::Tvd, *metres, place.tvd);
                    } else {
                        report.left(s, StatedField::Tvd, NotComparedWhy::TvdFromAnotherPoint);
                    }
                }
            },
        }

        // The displacements are relative to the well location, along the survey's own north.
        let scaled = stations.corrections.grid_scale_factor_stated;
        for (field, value, computed) in [
            (StatedField::DispNs, &stated.disp_ns, place.north),
            (StatedField::DispEw, &stated.disp_ew, place.east),
        ] {
            match value {
                None => {}
                Some(Err(fault)) => report.left(s, field, NotComparedWhy::Unit(fault.clone())),
                Some(Ok(_)) if scaled => report.left(s, field, NotComparedWhy::GridScaleFactor),
                Some(Ok(v)) => report.compared(s, field, *v, computed),
            }
        }

        // The vertical section goes through the one formula of the section, never a copy of it.
        match &stated.vert_sect {
            None => {}
            Some(Err(fault)) => report.left(s, StatedField::VertSect, NotComparedWhy::Unit(fault.clone())),
            Some(Ok(v)) => match section {
                None => report.left(s, StatedField::VertSect, NotComparedWhy::NoSection),
                Some(chosen) => {
                    let projection = VerticalSection {
                        azimuth: chosen.azimuth.to_radians(),
                        origin: chosen.origin,
                    };
                    report.compared(
                        s,
                        StatedField::VertSect,
                        *v,
                        projection.abscissa([place.east, place.north]),
                    );
                }
            },
        }

        // The course that ends at the station.
        match &stated.dls {
            None => {}
            Some(Err(fault)) => report.left(s, StatedField::Dls, NotComparedWhy::Unit(fault.clone())),
            Some(Ok(_)) if j == 0 => report.left(s, StatedField::Dls, NotComparedWhy::FirstStation),
            Some(Ok(v)) => report.compared(s, StatedField::Dls, *v, courses[j - 1].severity()),
        }
    }
    Ok(report)
}
