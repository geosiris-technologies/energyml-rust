//! An object as bytes, the way it travels: ETP's `DataObject`.
//!
//! A server keeps what it was sent and answers with it (the first thing an ETP
//! store gets right: the raw object as received, `serveur-etp.md` §1 of the
//! design notes). Deserialising every object on the way in and serialising it
//! again on the way out costs, and loses what the generated model does not keep.
//! So the traits carry the bytes too, next to the typed objects:
//! [`crate::EnergymlStorage::get_raw`] and [`crate::write::EnergymlWriter::put_raw`].

/// How the bytes are written: ETP's `format` (`xml` or `json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawFormat {
    /// The XML of the object, as the schemas define it.
    Xml,
    /// The JSON form ETP 1.2 allows. `TODO(verify)`: which JSON - the RDDMS
    /// convention or another (`serveur-etp-architecture.md` §12 of the design
    /// notes); a store that does not produce it refuses by name.
    Json,
}

impl RawFormat {
    /// The word ETP uses for it.
    pub fn as_etp(self) -> &'static str {
        match self {
            RawFormat::Xml => "xml",
            RawFormat::Json => "json",
        }
    }

    /// From the word ETP uses; `None` for any other.
    pub fn from_etp(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "xml" => Some(RawFormat::Xml),
            "json" => Some(RawFormat::Json),
            _ => None,
        }
    }
}

/// One object as bytes.
///
/// Built with [`RawObject::new`]; `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RawObject {
    /// The object's canonical URI.
    pub uri: String,
    /// How [`RawObject::bytes`] are written.
    pub format: RawFormat,
    /// The object, exactly as stored or as received.
    pub bytes: Vec<u8>,
}

impl RawObject {
    /// An object as bytes.
    pub fn new(uri: impl Into<String>, format: RawFormat, bytes: Vec<u8>) -> Self {
        Self {
            uri: uri.into(),
            format,
            bytes,
        }
    }
}
