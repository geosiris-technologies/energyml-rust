//! What can go wrong between an object and a set of points.

use std::fmt;

/// Result of a geometry operation.
pub type Result<T> = std::result::Result<T, Error>;

/// Something that stopped a representation from becoming points.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A shape the caller asked for that this object cannot take.
    ///
    /// Not a gap in this build: the file states no connection the answer could
    /// be derived from. Names both ends, because whoever asked wrote a rule and
    /// needs to know which of their rules met which object.
    #[error(
        "{topology} cannot be drawn as {wanted}: the file states no connection this could be \
         derived from"
    )]
    Shape {
        /// What the object is, in words - "a set of polylines".
        topology: &'static str,
        /// What was asked of it - "a surface".
        wanted: &'static str,
    },

    /// A form of array this crate does not evaluate yet.
    ///
    /// **Named, never silent.** The alternative - returning no points - looks
    /// exactly like a working program with an empty file, and that is the
    /// failure mode this crate exists to avoid.
    #[error("`{form}` is not evaluated yet ({at})")]
    Unsupported {
        /// The concrete type met, e.g. `Point3dParametricArray`.
        form: &'static str,
        /// Where it was met, as a dotted path.
        at: String,
    },

    /// The file states a shape with **no extent of its own**.
    ///
    /// Different news from [`Error::Unsupported`], and the difference is one a
    /// caller acts on. *Unsupported* means this build does not evaluate that
    /// form yet, so waiting is a strategy. This means the file **never says**
    /// how far the shape goes: a `HorizontalPlaneGeometry` is one number, a
    /// depth, and a plane is infinite. No release will add the extent, because
    /// it is not a fact about the model - it is a decision about the picture,
    /// and it belongs to whoever is drawing.
    ///
    /// So the refusal names what is missing rather than pretending to a gap,
    /// and a viewer can offer the one thing that works: an extent of its own.
    /// Drawing some box instead would put a surface of invented size into a
    /// model, which is the failure this crate exists to avoid.
    #[error("`{form}` has no extent of its own ({at}): {states}")]
    Unbounded {
        /// The concrete type met, e.g. `HorizontalPlaneGeometry`.
        form: &'static str,
        /// Where it was met, as a dotted path.
        at: String,
        /// What the file *does* state - the half that is there.
        states: &'static str,
    },

    /// The object is not shaped the way its schema says.
    #[error("{at}: {message}")]
    Malformed {
        /// Where, as a dotted path.
        at: String,
        /// What was expected and what was found.
        message: String,
    },

    /// The referenced object is not in the store.
    #[error("`{0}` is not in this store")]
    Missing(String),

    /// Reading an external array failed.
    #[error(transparent)]
    Storage(#[from] energyml_storage::Error),

    /// Reading or walking an object failed.
    #[error(transparent)]
    Core(#[from] energyml_core::error::Error),

    /// A unit, a coordinate system or a reprojection could not be read.
    #[error(transparent)]
    Domain(#[from] energyml_domain::Error),
}

impl Error {
    /// Whether this is "the external file is not there".
    ///
    /// A package whose `.h5` companion was not shipped is a gap in what the
    /// caller was given, not a defect in the model - and a viewer says so
    /// differently from "this build cannot evaluate that form".
    pub fn is_missing_file(&self) -> bool {
        match self {
            Error::Storage(energyml_storage::Error::ExternalData {
                reason: energyml_storage::ExternalDataReason::FileMissing,
                ..
            }) => true,
            Error::Domain(e) => e.is_missing_file(),
            _ => false,
        }
    }

    /// A shape the caller asked for that this object cannot take.
    ///
    /// Separate from [`Error::Unsupported`] on purpose, and the distinction is
    /// the whole message: that one means *this build does not do it yet*, and
    /// invites a future version. This one means *the file does not say enough
    /// for anyone to do it* - a set of polylines is not a surface in any build.
    pub fn shape(topology: &'static str, wanted: &'static str) -> Self {
        Self::Shape { topology, wanted }
    }

    /// A form this crate meets but does not evaluate.
    pub fn unsupported(form: &'static str, at: impl fmt::Display) -> Self {
        Self::Unsupported {
            form,
            at: at.to_string(),
        }
    }

    /// A shape whose extent the file does not state.
    pub fn unbounded(form: &'static str, at: impl fmt::Display, states: &'static str) -> Self {
        Self::Unbounded {
            form,
            at: at.to_string(),
            states,
        }
    }

    /// An object that does not match its schema.
    pub fn malformed(at: impl fmt::Display, message: impl fmt::Display) -> Self {
        Self::Malformed {
            at: at.to_string(),
            message: message.to_string(),
        }
    }
}
