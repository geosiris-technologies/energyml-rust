//! **A long profile over a package's wells** — the heads of the wells read in
//! one placement, projected on a drawn track.
//!
//! The arithmetic is [`crate::profile`] and the heads are
//! `energyml_geometry::heads`; this is the one place that joins them, so a face
//! (the command line, a WebAssembly front end) parses its arguments,
//! calls [`of_wells`] and prints — it decides nothing.
//!
//! The track is in the **placement** asked for, in that placement's unit; the
//! answer's [`WellsProfile::system`] is the EPSG code the heads share, for the
//! caller to compare with the system the track was drawn in.

use energyml_geometry::{Placement, ValueSource, heads};

use crate::profile::{self, Profile, ProfileError};

/// Why a profile could not be made.
#[derive(Debug, thiserror::Error)]
pub enum WellsProfileError {
    /// The track or a width is not usable.
    #[error(transparent)]
    Track(#[from] ProfileError),
    /// The heads could not be read as a whole (two systems, for one).
    #[error("{0}")]
    Heads(String),
}

/// The wells of a package on a track.
#[derive(Debug, Clone, PartialEq)]
pub struct WellsProfile {
    /// The projection: kept and rejected, by id (the object's uri as asked).
    pub profile: Profile,
    /// The polygon of the band, counter-clockwise, not closed.
    pub band: Vec<[f64; 2]>,
    /// Length of the track.
    pub length: f64,
    /// The citation title of each head read, by id.
    pub titles: Vec<(String, String)>,
    /// Objects that gave no head, with the reason: `(id, why)`.
    pub refused: Vec<(String, String)>,
    /// The EPSG code the heads share, when their CRS states one.
    pub system: Option<u64>,
    /// How many heads were read.
    pub heads: usize,
}

impl WellsProfile {
    /// The title of a well, or an empty string.
    pub fn title(&self, id: &str) -> &str {
        self.titles
            .iter()
            .find(|(one, _)| one == id)
            .map(|(_, title)| title.as_str())
            .unwrap_or("")
    }
}

/// Projects `asked` (wellbore trajectories or frames) on `track` — plan
/// `[x, y]` in `placement` — within `left` / `right`. `arc_step` is the angle in
/// radians a rounded outer turn of the band is cut at.
///
/// # Errors
///
/// A track that is not a track, a width that is not a number, or heads in two
/// systems. One object that is not a well goes to [`WellsProfile::refused`].
pub fn of_wells(
    source: &dyn ValueSource,
    asked: &[String],
    track: &[[f64; 2]],
    left: f64,
    right: f64,
    arc_step: f64,
    placement: Placement,
) -> Result<WellsProfile, WellsProfileError> {
    let read =
        heads::of_objects(source, asked, placement).map_err(|e| WellsProfileError::Heads(e.to_string()))?;
    let batch: Vec<profile::Head> = read
        .heads
        .iter()
        .map(|h| profile::Head::new(h.id.clone(), h.xy))
        .collect();
    let section = profile::project(track, left, right, &batch, arc_step)?;
    Ok(WellsProfile {
        profile: section.profile,
        band: section.band,
        length: section.length,
        titles: read
            .heads
            .iter()
            .map(|h| (h.id.clone(), h.title.clone()))
            .collect(),
        refused: read
            .refused
            .iter()
            .map(|r| (r.id.clone(), r.why.clone()))
            .collect(),
        system: read.system,
        heads: read.heads.len(),
    })
}
