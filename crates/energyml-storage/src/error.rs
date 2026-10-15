//! What can go wrong, and what merely looks odd.
//!
//! Two distinct notions live here and must not be confused. An [`Error`] stops
//! one operation. A [`Diagnostic`] does not stop anything: it records that the
//! store was read anyway, despite something the specification does not allow.
//!
//! Files in the wild are not conformant - the reference package published with
//! fesapi declares one object twice in `[Content_Types].xml`, with a content
//! type that belongs to a different part. A reader that refuses such a package
//! is useless; a reader that swallows the problem is worse. So: read it, and
//! say so.

use std::fmt;

/// Result of a storage operation.
pub type Result<T> = std::result::Result<T, Error>;

/// Something that stopped an operation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No object with this URI in the store.
    #[error("no object at `{0}`")]
    NotFound(String),

    /// A short URI that names more than one object: the same uuid and type
    /// held under several `objectVersion`s. Identifier Specification v5.0
    /// §3.4.2.1 (p. 15) leaves "the most recent" to the server and ETP 1.2 says
    /// nothing of how to tell; this store does not guess, it names the
    /// candidates - each spelled with its version, so any one of them resolves.
    #[error("`{uri}` names {} objects; name the version: {}", candidates.len(), candidates.join(", "))]
    Ambiguous {
        /// The URI as given.
        uri: String,
        /// Every object it could mean, each by its full URI.
        candidates: Vec<String>,
    },

    /// The URI could not be parsed, or names something this store cannot hold.
    #[error("`{uri}` is not usable here: {reason}")]
    BadUri {
        /// The URI as given.
        uri: String,
        /// Why it was rejected.
        reason: String,
    },

    /// A part of something asked for that is not inside it: a window of an
    /// array starting or reaching past an axis, a start or count whose rank is
    /// not the array's. The request is wrong, not the data.
    #[error("`{location}`: {reason}")]
    OutOfBounds {
        /// What was asked into, by its address (an array: `uri` plus path).
        location: String,
        /// The figures: which axis, which start, which length.
        reason: String,
    },

    /// A request larger than the bound its caller set: answered by naming the
    /// size, never by returning a part of it as if it were the whole.
    #[error("`{location}`: {reason}")]
    TooLarge {
        /// What was asked for, by its address.
        location: String,
        /// The size found and the bound, so the caller knows what to narrow.
        reason: String,
    },

    /// The content type names a type no registered crate declares.
    #[error("unknown type `{0}` - is the generated crate registered?")]
    UnknownType(String),

    /// The package itself is malformed beyond what tolerance can absorb.
    #[error("package error: {0}")]
    Package(String),

    /// An operation this implementation does not offer.
    ///
    /// Not a failure of the data: a capability the transport does not have.
    #[error("{operation} is not supported by {store}")]
    Unsupported {
        /// What was attempted.
        operation: &'static str,
        /// Which implementation refused.
        store: &'static str,
    },

    /// A request by relationship kind met an edge whose kind nobody states.
    ///
    /// ETP 1.2 section 8.1.1.1.2 (p. 157) leaves the kind of a relationship to
    /// the ML of the object holding it, and the guide of an ML that has none
    /// says nothing: the edge has no kind, and the request that needs one is
    /// refused **when it meets that edge**, not before
    /// ([`crate::context::discover`]).
    #[error("`{source_type}`: the kind of its edges is not stated: {why}")]
    UnstatedKind {
        /// The qualified type of the object holding the reference
        /// (`prodml23.FlowTestActivity`), or the URI when it has none.
        source_type: String,
        /// Why no rule applies ([`crate::kinds::KindBasis::Unknown`]).
        why: &'static str,
    },

    /// A transaction was misused: a second one opened while one is, or one
    /// ended that is not the open one (ETP protocol 18).
    #[error("{store}: {reason}")]
    Transaction {
        /// Which implementation refused.
        store: &'static str,
        /// What was wrong.
        reason: String,
    },

    /// An external data file could not be reached or read.
    #[error("external data `{location}`: {reason}")]
    ExternalData {
        /// File the array lives in, as named by the object or its relationship.
        location: String,
        /// Why it could not be read.
        reason: ExternalDataReason,
    },

    /// Reading or writing the underlying bytes failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A remote store failed as a whole - the session fell, the server
    /// refused the request. What the server said of **one** object or array
    /// comes back as that object's own error instead.
    #[error("{store}: {reason}")]
    Remote {
        /// Which kind of store, as [`crate::storage::EnergymlStorage::store_kind`] says.
        store: &'static str,
        /// What went wrong, in the server's words when it gave some.
        reason: String,
    },

    /// The XML of a part could not be read.
    #[error("XML error: {0}")]
    Xml(String),

    /// Anything raised by `energyml-core` while reading or writing an object.
    #[error(transparent)]
    Core(#[from] energyml_core::error::Error),
}

/// Why an array could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExternalDataReason {
    /// The support for this format is not compiled in.
    ///
    /// A capability that is missing, **not** data that is absent - the caller
    /// must be able to tell the two apart, which is why this is not a plain
    /// "not found". Enable the matching feature, or supply the file yourself.
    FeatureNotEnabled {
        /// The Cargo feature that would have handled it, e.g. `hdf5`.
        feature: &'static str,
        /// MIME type the object declared, when it declared one.
        mime: Option<String>,
    },
    /// The file is named by the package but nothing was found under that name.
    FileMissing,
    /// The file was found but the dataset path inside it was not.
    PathMissing(String),
    /// The file identifies itself with a different uuid than the package says.
    ///
    /// The EPC specification requires an HDF5 file to carry, at its root, a
    /// `uuid` attribute equal to that of its `EpcExternalPartReference`. A
    /// mismatch means this is somebody else's file.
    UuidMismatch {
        /// What the package expected.
        expected: String,
        /// What the file claims.
        found: String,
    },
    /// The backend failed for a reason of its own.
    Backend(String),
    /// The shape and the values of an array being written disagree.
    ///
    /// Checked before the backend is asked, because a shape that promises more
    /// numbers than there are **writes without error** in some formats and
    /// reads back with zeros at the end - plausible values that nothing flags.
    /// Both counts are named: it is their difference that says what to fix.
    ShapeDisagrees {
        /// How many the shape promises.
        promised: i64,
        /// How many the block holds.
        held: usize,
    },
}

impl fmt::Display for ExternalDataReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FeatureNotEnabled { feature, mime } => match mime {
                Some(mime) => write!(f, "no support for `{mime}` (build with the `{feature}` feature)"),
                None => write!(f, "no support compiled in (build with the `{feature}` feature)"),
            },
            Self::FileMissing => write!(f, "file not found"),
            Self::PathMissing(path) => write!(f, "no dataset at `{path}` in the file"),
            Self::UuidMismatch { expected, found } => {
                write!(
                    f,
                    "the file carries uuid `{found}`, the package expects `{expected}`"
                )
            }
            Self::Backend(reason) => write!(f, "{reason}"),
            Self::ShapeDisagrees { promised, held } => write!(
                f,
                "the shape promises {promised} value(s) and the block holds {held}"
            ),
        }
    }
}

/// How much a diagnostic matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The package departs from the specification; the reader coped.
    Warning,
    /// Worth knowing, no departure. A fallback was taken, a default applied.
    Note,
}

/// Something noticed while reading, which did not stop anything.
///
/// Every departure from the specification produces one. The caller decides
/// whether to ignore them, show them, or treat them as fatal - but they are
/// never dropped in silence, which is the same rule the code generator follows
/// for the schemas it cannot map.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// How much it matters.
    pub severity: Severity,
    /// Which part of the package it concerns, empty for the package itself.
    pub part: String,
    /// What was noticed, in one sentence.
    pub message: String,
}

impl Diagnostic {
    /// A departure from the specification.
    pub fn warning(part: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            part: part.into(),
            message: message.into(),
        }
    }

    /// A choice worth recording.
    pub fn note(part: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Note,
            part: part.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.severity {
            Severity::Warning => "warning",
            Severity::Note => "note",
        };
        if self.part.is_empty() {
            write!(f, "[{tag}] {}", self.message)
        } else {
            write!(f, "[{tag}] {}: {}", self.part, self.message)
        }
    }
}
