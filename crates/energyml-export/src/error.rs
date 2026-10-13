//! What can go wrong writing geometry out.

use thiserror::Error;

/// The result of an export.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything an exporter refuses to do rather than do wrongly.
#[derive(Debug, Error)]
pub enum Error {
    /// No exporter in the registry writes this format.
    #[error("no exporter for `{name}` (this build has: {available})")]
    UnknownFormat {
        /// What was asked for.
        name: String,
        /// What the registry holds, comma separated.
        available: String,
    },

    /// The object cannot be written in this format.
    ///
    /// A point cloud has no place in a format that only carries triangles, and
    /// writing an empty file would be worse than saying so.
    #[error("{format} cannot write {what}")]
    Unsuitable {
        /// The format that refused.
        format: &'static str,
        /// What it was given.
        what: String,
    },

    /// The coordinates could not be put where the format needs them.
    #[error("{0}")]
    Placement(String),

    /// Reading the geometry failed.
    #[error(transparent)]
    Geometry(#[from] energyml_geometry::Error),

    /// A unit, a coordinate system or a reprojection failed.
    #[error(transparent)]
    Domain(#[from] energyml_domain::Error),

    /// Writing failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// This format cannot represent that.
    pub fn unsuitable(format: &'static str, what: impl Into<String>) -> Self {
        Error::Unsuitable {
            format,
            what: what.into(),
        }
    }
}
