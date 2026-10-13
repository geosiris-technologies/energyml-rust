//! An object's own frame, and how its points reach the scene's
//! (ADR-0015).
//!
//! Used for wells too. Contract:
//! [`ObjectFrame`], [`frame_of`], [`to_scene`].
//!
//! # One reading, not a second one
//!
//! The scene's own answers are a front end's CRS lookup (an object's CRS, then
//! `reproject::System::of`) and a front end's drawable `verticalIsTime`
//! (`Crs::is_time`). This module asks the same two library functions —
//! `crs::of` and `System::of` — rather than reading a CRS a second way: the
//! section view and the scene must agree on which system an object is in, or
//! one of them draws a model where it is not.
//!
//! # What is refused, by name
//!
//! Nothing is guessed: a point in an unknown system is not "probably the
//! scene's" (a Lambert-93 model read as UTM looks right and is 600 km away).
//!
//! - the object names a system and the scene has none, or the reverse;
//! - an object in time and no `time_factor` — which says how many metres of
//!   depth a unit of time is worth in this scene;
//! - whatever the reprojection backend refuses (a datum it does not know, a
//!   unit it cannot convert), in the backend's own words.

use energyml_core::value::Reflect;
use energyml_domain::ArraySource as ValueSource;
use energyml_domain::reproject::{self, System as DomainSystem};

use crate::model::System;
use crate::read::Scene;

/// What an object's points are in: its horizontal system (`None` when its CRS
/// names none) and whether its vertical axis is time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectFrame {
    /// The horizontal system its placed points are in.
    pub system: Option<System>,
    /// Its vertical axis is a time, not a length.
    pub time: bool,
}

/// The frame of a representation, read from its CRS — the same reading the
/// scene makes (`workspace.systemOf`), never a second one.
///
/// An object that names **no** CRS has no system and no time: its local
/// coordinates are the only ones there are, and that is [`to_scene`]'s to judge
/// against the scene. A CRS that names no EPSG code gives no system either —
/// `System::of` refuses it, and for the scene that is the same fact (nothing to
/// reproject from), said by [`to_scene`] when it matters. A CRS that cannot be
/// read at all is an error, never an absent frame.
pub fn frame_of(source: &dyn ValueSource, representation: &dyn Reflect) -> Result<ObjectFrame, String> {
    let Some(crs) = energyml_domain::crs::of(representation, source).map_err(|why| format!("{why}"))? else {
        return Ok(ObjectFrame::default());
    };
    let system = DomainSystem::of(&crs).ok().map(|found| System {
        epsg: found.epsg,
        unit: found.unit,
    });
    Ok(ObjectFrame {
        system,
        time: crs.is_time(),
    })
}

/// Bring placed points into the scene: reproject `[x, y]` when the object's
/// system is not the scene's, and carry a time onto the depth axis by the
/// scene's time factor. Each refusal names what is missing (no system on one
/// side, no time factor for an object in time, a backend that refuses).
///
/// The points are **placed** ones (`Placement::Projected`): the CRS's offset,
/// rotation and vertical flip are already applied, which is what
/// `reproject_placed` wants — applying them twice is the bug that function
/// exists to make impossible. On error the points are left as they were.
pub fn to_scene(points: &mut [[f64; 3]], frame: &ObjectFrame, scene: &Scene<'_>) -> Result<(), String> {
    // The time first, since it is a refusal that needs no work: nothing is
    // reprojected for an object that will be refused anyway.
    let factor = match (frame.time, scene.time_factor) {
        (false, _) => None,
        (true, Some(factor)) => Some(factor),
        (true, None) => {
            return Err(
                "the object is in time and the scene has no time factor (the metres of depth a unit of \
                 time is worth), so it cannot be carried onto the depth axis"
                    .to_string(),
            );
        }
    };

    match (&frame.system, scene.system) {
        (None, None) => {}
        (Some(own), None) => {
            return Err(format!(
                "the object is in EPSG:{} ({}) and the scene names no system: it would be drawn against \
                 coordinates it does not share",
                own.epsg, own.unit
            ));
        }
        (None, Some(target)) => {
            return Err(format!(
                "the object names no horizontal system (no CRS, or a CRS with no EPSG code) and the scene \
                 is in EPSG:{}: there is nothing to reproject it from",
                target.epsg
            ));
        }
        (Some(own), Some(target)) => {
            if own != target {
                let from = DomainSystem {
                    epsg: own.epsg,
                    unit: own.unit.clone(),
                };
                let to = DomainSystem {
                    epsg: target.epsg,
                    unit: target.unit.clone(),
                };
                let moved =
                    reproject::reproject_placed(points, &from, &to, scene.reprojector).map_err(|why| {
                        format!(
                            "reprojecting from EPSG:{} to EPSG:{}: {why}",
                            own.epsg, target.epsg
                        )
                    })?;
                points.copy_from_slice(&moved);
            }
        }
    }

    if let Some(factor) = factor {
        for point in points.iter_mut() {
            point[2] *= factor;
        }
    }
    Ok(())
}

/// The system of the first of these objects that names one — what a face with
/// a single package and no scene (the CLI) takes as the scene's system.
/// `None` when none of them names one.
pub fn first_system(source: &dyn ValueSource, uris: &[String]) -> Option<System> {
    uris.iter().find_map(|uri| {
        // By the uri: it names one version of an object, the uuid may not.
        let object = source.object_at(uri)?;
        frame_of(source, object.as_reflect()).ok()?.system
    })
}
