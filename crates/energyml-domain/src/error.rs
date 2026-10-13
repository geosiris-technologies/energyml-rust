//! What can go wrong reading domain values.

use thiserror::Error;

/// The result of a domain read.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything this crate refuses to guess.
///
/// Each variant names what was missing rather than returning a default: a
/// coordinate system that quietly falls back to metres, or a unit assumed to
/// be the base one, produces a model in the wrong place with nothing to show
/// for it.
#[derive(Debug, Error)]
pub enum Error {
    /// A unit outside the conversion table.
    ///
    /// Not "unitless" and not the base unit — unknown. The `*UomExt` enums are
    /// open, so a producer can write anything.
    #[error("`{unit}` is not a unit this build converts (see energyml-domain::units)")]
    UnknownUnit {
        /// The unit as written in the file.
        unit: String,
    },

    /// Two units that do not measure the same thing.
    #[error("cannot convert `{from}` to `{to}`: they are different kinds of quantity")]
    MismatchedUnits {
        /// The unit converted from.
        from: String,
        /// The unit converted to.
        to: String,
    },

    /// A unit that is valid but of the wrong family for where it was used.
    #[error("`{unit}` is a {found} unit, but a {expected} was expected here")]
    WrongDimension {
        /// The unit as written in the file.
        unit: String,
        /// The family it belongs to.
        found: String,
        /// The family the caller needed.
        expected: String,
    },

    /// An object a reference names is not in the store.
    #[error("{0} is referenced but not in the store")]
    Missing(String),

    /// A file whose structure contradicts itself.
    #[error("{at}: {reason}")]
    Malformed {
        /// Where in the object, as a field path.
        at: String,
        /// What was wrong with it.
        reason: String,
    },

    /// Reading an external array failed.
    #[error(transparent)]
    Storage(#[from] energyml_storage::Error),

    /// A shape this build does not evaluate.
    ///
    /// Always names the concrete thing: silence here is indistinguishable from
    /// data that is not present.
    #[error("{what} is not supported yet ({detail})")]
    Unsupported {
        /// The construct that was met.
        what: String,
        /// Where it was met.
        detail: String,
    },
}

impl Error {
    /// Whether this is "the external file is not there".
    ///
    /// Worth its own question rather than a match on variants: a caller shows
    /// a missing companion file differently from a form it cannot evaluate,
    /// and the error now travels through two layers of wrapping, so matching
    /// by hand gets it wrong the first time something is re-wrapped.
    pub fn is_missing_file(&self) -> bool {
        matches!(
            self,
            Error::Storage(energyml_storage::Error::ExternalData {
                reason: energyml_storage::ExternalDataReason::FileMissing,
                ..
            })
        )
    }

    /// Whether this is "no backend in this build can read that file".
    ///
    /// Distinct from a missing file, and a caller must be able to tell them
    /// apart: one is a package that arrived incomplete, the other is a build
    /// compiled without the `hdf5` feature. Both leave the values unread; only
    /// one is fixable by finding the file.
    pub fn needs_array_backend(&self) -> bool {
        matches!(
            self,
            Error::Storage(energyml_storage::Error::ExternalData {
                reason: energyml_storage::ExternalDataReason::FeatureNotEnabled { .. },
                ..
            })
        )
    }

    /// A unit outside the table.
    pub fn unknown_unit(unit: impl Into<String>) -> Self {
        Error::UnknownUnit { unit: unit.into() }
    }

    /// A file that contradicts itself.
    pub fn malformed(at: impl Into<String>, reason: impl Into<String>) -> Self {
        Error::Malformed {
            at: at.into(),
            reason: reason.into(),
        }
    }

    /// A construct this build does not handle.
    pub fn unsupported(what: impl Into<String>, detail: impl Into<String>) -> Self {
        Error::Unsupported {
            what: what.into(),
            detail: detail.into(),
        }
    }
}
