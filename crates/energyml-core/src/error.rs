//! Error type shared by serialization, reflection and validation.

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while reading, writing or reflecting over an
/// EnergyML object.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Low level XML failure reported by the parser or writer.
    #[error("XML error: {0}")]
    Xml(String),

    /// I/O failure while reading or writing.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The document ended while a value was still being read.
    #[error("unexpected end of document")]
    Eof,

    /// An element was found where another one was required.
    #[error("expected element <{expected}>, found <{found}>")]
    UnexpectedElement {
        /// Element name the reader was looking for.
        expected: String,
        /// Element name actually encountered.
        found: String,
    },

    /// A mandatory attribute or child element was absent.
    #[error("missing required {kind} `{name}` in <{parent}>")]
    MissingRequired {
        /// `"attribute"` or `"element"`.
        kind: &'static str,
        /// Name of the missing item.
        name: String,
        /// Element that should have contained it.
        parent: String,
    },

    /// A lexical value could not be converted to its Rust type.
    #[error("cannot parse `{value}` as {ty}: {reason}")]
    ParseValue {
        /// The offending lexical value.
        value: String,
        /// Target type name.
        ty: &'static str,
        /// Why the conversion failed.
        reason: String,
    },

    /// An `xsi:type` (or substituted element name) did not match any known
    /// concrete subtype of the expected abstract type.
    #[error("unknown xsi:type `{value}` for abstract type `{ty}`")]
    UnknownXsiType {
        /// The `xsi:type` value read from the document.
        value: String,
        /// Abstract type being resolved.
        ty: &'static str,
    },

    /// Reflection was asked for a field the type does not declare.
    #[error("type `{ty}` has no field `{field}`")]
    UnknownField {
        /// Type being reflected over.
        ty: &'static str,
        /// Requested field name.
        field: String,
    },

    /// A dotted path could not be parsed.
    #[error("invalid path `{path}`: {reason}")]
    InvalidPath {
        /// The path as supplied by the caller.
        path: String,
        /// Why it is rejected.
        reason: String,
    },

    /// A value of the wrong shape was supplied to a reflective setter.
    #[error("type mismatch at `{path}`: expected {expected}, found {found}")]
    TypeMismatch {
        /// Location of the mismatch.
        path: String,
        /// Expected kind or type name.
        expected: String,
        /// Kind or type name actually supplied.
        found: String,
    },

    /// The registry does not know the requested type.
    #[error("unknown type `{0}` (not registered)")]
    UnknownType(String),

    /// The requested type belongs to a schema version the project knows but
    /// this build does not carry (see `Registry::declare_absent`). A refusal
    /// with its reason, not an unknown name: the data may be perfectly good.
    /// The message is the whole story and says what would bring the version.
    #[error("{0}")]
    VersionAbsent(String),

    /// Catch-all for anything not worth its own variant.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Convenience constructor for [`Error::Other`].
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }

    /// Convenience constructor for [`Error::ParseValue`].
    pub fn parse(value: impl Into<String>, ty: &'static str, reason: impl Into<String>) -> Self {
        Error::ParseValue {
            value: value.into(),
            ty,
            reason: reason.into(),
        }
    }
}

impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::Xml(e.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for Error {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        Error::Xml(e.to_string())
    }
}

impl From<quick_xml::encoding::EncodingError> for Error {
    fn from(e: quick_xml::encoding::EncodingError) -> Self {
        Error::Xml(e.to_string())
    }
}

impl From<quick_xml::escape::EscapeError> for Error {
    fn from(e: quick_xml::escape::EscapeError) -> Self {
        Error::Xml(e.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Self {
        Error::Xml(format!("invalid UTF-8: {e}"))
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Error::Xml(format!("invalid UTF-8: {e}"))
    }
}
