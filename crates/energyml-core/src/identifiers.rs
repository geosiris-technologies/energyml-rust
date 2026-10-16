//! Energistics identifiers: UUID, content type, qualified type and ETP URI.
//!
//! See `doc/Energistics_Identifier_Specification_v4.0_Doc_v1.2.pdf`. These are
//! parsed with regexes because the same strings appear in `[Content_Types].xml`,
//! in `.rels`, in DORs and in ETP messages, and all of them must agree.

use std::fmt;
use std::sync::OnceLock;

use regex::Regex;

use crate::error::{Error, Result};

const UUID_PATTERN: &str = r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!("^{UUID_PATTERN}$")).expect("valid uuid regex"))
}

fn content_type_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            ^\s*
            application/
            (?:x-(?<domain>[\w]+)\+xml|(?<mime>[\w.+-]+))
            \s*;\s*version=(?<version>[^;\s]+)
            (?:\s*;\s*type=(?<type>[\w]+))?
            \s*$",
        )
        .expect("valid content type regex")
    })
}

fn qualified_type_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s*(?<domain>[a-zA-Z]+)(?<version>[\d]+)\.(?<type>[\w]+)\s*$")
            .expect("valid qualified type regex")
    })
}

/// A quoted value of an ETP URI - a dataspace path, an object version - where
/// a quote is written twice (ETP v1.2 §25.3.10).
const QUOTED_PATTERN: &str = r"[^']*?(?:''[^']*?)*";

fn etp_uri_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?x)
            ^\s*eml:///
            (?:dataspace\('(?<dataspace>{QUOTED_PATTERN})'\)/?)?
            (?:
                (?<domain>[a-zA-Z]+)(?<version>[\d]+)
                \.(?<type>[\w]+)
                \((?:uuid=)?(?<uuid>{UUID_PATTERN})(?:,version='(?<objversion>{QUOTED_PATTERN})')?\)
                (?:/(?<rest>.*))?
            )?
            \s*$"
        ))
        .expect("valid ETP URI regex")
    })
}

/// The four regular expressions ETP v1.2 gives for a **canonical** URI
/// (§25.3.10; the Identifier Specification v5.0 repeats them): a dataspace, a
/// data object with or without its version, and the two query forms.
///
/// The reader above is lenient - it takes `(uuid=…)`, an alternate form - so
/// whether a URI is canonical is a question of its own, answered here and
/// nowhere else.
fn canonical_etp_uri_res() -> &'static [Regex; 4] {
    static RES: OnceLock<[Regex; 4]> = OnceLock::new();
    RES.get_or_init(|| {
        let dataspace = format!(r"(?:dataspace\('{QUOTED_PATTERN}'\)/)?");
        let object = format!(
            r"(?:witsml|resqml|prodml|eml)[1-9]\d\.\w+\((?:{UUID_PATTERN}|uuid={UUID_PATTERN},version='{QUOTED_PATTERN}')\)"
        );
        let collection = r"(?:witsml|resqml|prodml|eml)[1-9]\d\.\w+(?:\?[^#]+)?";
        [
            format!(r"^eml:///(?:dataspace\('{QUOTED_PATTERN}'\))?$"),
            format!(r"^eml:///{dataspace}{object}$"),
            format!(r"^eml:///{dataspace}{object}/{collection}$"),
            format!(r"^eml:///{dataspace}{collection}$"),
        ]
        .map(|pattern| Regex::new(&pattern).expect("valid canonical ETP URI regex"))
    })
}

/// Whether a string is a well-formed UUID.
pub fn is_uuid(value: &str) -> bool {
    uuid_re().is_match(value.trim())
}

/// Whether a string is the nil UUID, `00000000-0000-0000-0000-000000000000`,
/// in any letter case and ignoring surrounding whitespace.
///
/// The nil UUID matches the `UuidString` pattern of the schemas, so
/// [`is_uuid`] accepts it. It is the special form of RFC 4122 section 4.1.7
/// ("all 128 bits set to zero"), and it identifies no instance: Energistics
/// Identifier Specification v5.0, section 2.3.1 (p. 9), requires that "a unique
/// instances of business object defined by Energistics data models must be
/// identified with a UUID as defined by RFC 4122", and two objects of one type
/// that carry the nil UUID cannot be told apart. This is a detection rule, to
/// be reported; nothing refuses it.
pub fn is_nil_uuid(value: &str) -> bool {
    let value = value.trim();
    is_uuid(value) && value.bytes().all(|b| b == b'0' || b == b'-')
}

/// Validate and normalize a UUID to lowercase.
pub fn normalize_uuid(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if is_uuid(trimmed) {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Err(Error::parse(trimmed, "uuid", "not a UUID"))
    }
}

/// A parsed EPC content type, e.g.
/// `application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ContentType {
    /// Domain short name: `resqml`, `witsml`, `prodml`, `eml`.
    pub domain: String,
    /// Domain version as written, e.g. `2.2`.
    pub version: String,
    /// Object type **verbatim**, `obj_` prefix included where RESQML 2.0.1 and
    /// common 2.0 write one. Absent for non-object parts.
    ///
    /// Stripping the prefix here would corrupt the string on the way back out;
    /// use [`crate::naming::strip_obj_prefix`] when grouping instead.
    pub object_type: Option<String>,
    /// Raw media type when it is not an EnergyML `x-*+xml` type (a PDF or CSV
    /// carried along inside the EPC, for instance).
    pub raw_mime: Option<String>,
}

impl ContentType {
    /// Parse a content type string.
    pub fn parse(value: &str) -> Result<Self> {
        let caps = content_type_re()
            .captures(value)
            .ok_or_else(|| Error::parse(value, "contentType", "does not match the Energistics form"))?;
        Ok(ContentType {
            domain: caps
                .name("domain")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            version: caps["version"].to_string(),
            object_type: caps.name("type").map(|m| m.as_str().to_string()),
            raw_mime: caps.name("mime").map(|m| m.as_str().to_string()),
        })
    }

    /// Grouping key: the object type without its `obj_` prefix.
    pub fn type_key(&self) -> Option<&str> {
        self.object_type.as_deref().map(crate::naming::strip_obj_prefix)
    }

    /// Build the content type of a top-level object.
    pub fn of_object(domain: &str, version: &str, object_type: &str) -> String {
        format!("application/x-{domain}+xml;version={version};type={object_type}")
    }
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.raw_mime, &self.object_type) {
            (Some(mime), _) => write!(f, "application/{};version={}", mime, self.version),
            (None, Some(ty)) => {
                write!(
                    f,
                    "application/x-{}+xml;version={};type={}",
                    self.domain, self.version, ty
                )
            }
            (None, None) => write!(f, "application/x-{}+xml;version={}", self.domain, self.version),
        }
    }
}

/// A parsed qualified type, e.g. `resqml22.TriangulatedSetRepresentation`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QualifiedType {
    /// Domain short name.
    pub domain: String,
    /// Compact version digits as they appear in the string, e.g. `22`.
    pub version: String,
    /// Object type **verbatim**, `obj_` prefix included where the schema has
    /// one - `resqml20.obj_Activity` keeps it.
    pub object_type: String,
}

impl QualifiedType {
    /// Parse a qualified type string.
    pub fn parse(value: &str) -> Result<Self> {
        let caps = qualified_type_re()
            .captures(value)
            .ok_or_else(|| Error::parse(value, "qualifiedType", "expected `<domain><version>.<Type>`"))?;
        Ok(QualifiedType {
            domain: caps["domain"].to_string(),
            version: caps["version"].to_string(),
            object_type: caps["type"].to_string(),
        })
    }

    /// Grouping key: the object type without its `obj_` prefix.
    pub fn type_key(&self) -> &str {
        crate::naming::strip_obj_prefix(&self.object_type)
    }

    /// Dotted version, e.g. `22` becomes `2.2`.
    ///
    /// Energistics writes versions without separators in qualified types but
    /// with them in content types, and the two have to be matched up.
    pub fn dotted_version(&self) -> String {
        self.version
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
}

impl fmt::Display for QualifiedType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}.{}", self.domain, self.version, self.object_type)
    }
}

/// A parsed ETP 1.2 URI.
///
/// Covers the dataspace and data-object forms, the object's version included:
/// `eml:///dataspace('demo')/resqml22.TriangulatedSetRepresentation(uuid)`,
/// `eml:///resqml22.TriangulatedSetRepresentation(uuid=…,version='2')`.
///
/// An Energistics URI always has three slashes here: the Identifier
/// Specification v4.0 allowed an authority between the second and the third
/// (`eml://witsml14/well(uid)`), and v5.0 and ETP v1.2 no longer do.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EtpUri {
    /// Dataspace name, absent for the default dataspace.
    pub dataspace: Option<String>,
    /// Domain short name, absent for a bare dataspace URI.
    pub domain: Option<String>,
    /// Compact version digits.
    pub version: Option<String>,
    /// Object type.
    pub object_type: Option<String>,
    /// Object UUID, normalized to lowercase.
    pub uuid: Option<String>,
    /// The object's version, `version='…'`, when the URI names one - not to be
    /// confused with [`EtpUri::version`], the schema version of its type.
    pub object_version: Option<String>,
    /// Anything trailing the object, such as a sub-path.
    pub rest: Option<String>,
}

impl EtpUri {
    /// Parse an ETP URI.
    pub fn parse(value: &str) -> Result<Self> {
        let caps = etp_uri_re()
            .captures(value)
            .ok_or_else(|| Error::parse(value, "ETP URI", "does not match the ETP 1.2 grammar"))?;
        Ok(EtpUri {
            dataspace: caps.name("dataspace").map(|m| m.as_str().to_string()),
            domain: caps.name("domain").map(|m| m.as_str().to_string()),
            version: caps.name("version").map(|m| m.as_str().to_string()),
            object_type: caps.name("type").map(|m| m.as_str().to_string()),
            uuid: caps.name("uuid").map(|m| m.as_str().to_ascii_lowercase()),
            // A quote is written twice inside a quoted value (ETP v1.2 §25.3.10):
            // the version `O'Neil` travels as `O''Neil`.
            object_version: caps.name("objversion").map(|m| m.as_str().replace("''", "'")),
            rest: caps
                .name("rest")
                .map(|m| m.as_str().to_string())
                .filter(|s| !s.is_empty()),
        })
    }

    /// Whether `value` is a **canonical** ETP URI (ETP v1.2 §25.3.4-25.3.10) -
    /// what every endpoint must understand, and what some messages require
    /// whatever was negotiated (§3.7.4.2). Anything else, however well it
    /// reads, is at best an alternate URI (§25.3.9).
    pub fn is_canonical(value: &str) -> bool {
        canonical_etp_uri_res().iter().any(|re| re.is_match(value))
    }

    /// Whether the URI addresses a dataspace rather than an object.
    pub fn is_dataspace(&self) -> bool {
        self.uuid.is_none()
    }

    /// The qualified type this URI refers to, if it addresses an object.
    pub fn qualified_type(&self) -> Option<QualifiedType> {
        Some(QualifiedType {
            domain: self.domain.clone()?,
            version: self.version.clone()?,
            object_type: self.object_type.clone()?,
        })
    }

    /// Build a data-object URI.
    pub fn object(dataspace: Option<&str>, qualified_type: &str, uuid: &str) -> String {
        Self::object_versioned(dataspace, qualified_type, uuid, None)
    }

    /// Build a data-object URI that names the object's version when it has
    /// one: `eml:///{type}(uuid={uuid},version='{version}')` - Identifier
    /// Specification v5.0 §3.4.2 (p. 14); a quote inside the version is written
    /// twice (ETP v1.2 §25.3.10). Without a version this is the short form,
    /// which §3.4.2.1 (p. 15) reads as *the most recent version*.
    pub fn object_versioned(
        dataspace: Option<&str>,
        qualified_type: &str,
        uuid: &str,
        object_version: Option<&str>,
    ) -> String {
        let name = match object_version {
            Some(version) => {
                format!(
                    "{qualified_type}(uuid={uuid},version='{}')",
                    version.replace('\'', "''")
                )
            }
            None => format!("{qualified_type}({uuid})"),
        };
        match dataspace {
            Some(ds) => format!("eml:///dataspace('{ds}')/{name}"),
            None => format!("eml:///{name}"),
        }
    }
}

impl fmt::Display for EtpUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "eml:///")?;
        if let Some(ds) = &self.dataspace {
            write!(f, "dataspace('{ds}')")?;
            if self.uuid.is_some() {
                write!(f, "/")?;
            }
        }
        if let (Some(d), Some(v), Some(t), Some(u)) =
            (&self.domain, &self.version, &self.object_type, &self.uuid)
        {
            match &self.object_version {
                Some(ov) => write!(f, "{d}{v}.{t}(uuid={u},version='{}')", ov.replace('\'', "''"))?,
                None => write!(f, "{d}{v}.{t}({u})")?,
            }
            if let Some(rest) = &self.rest {
                write!(f, "/{rest}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids() {
        assert!(is_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(!is_uuid("not-a-uuid"));
        assert_eq!(
            normalize_uuid(" A1B2C3D4-E5F6-7890-ABCD-EF1234567890 ").unwrap(),
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        );
    }

    /// RFC 4122 section 4.1.7: the nil UUID is "all 128 bits set to zero";
    /// Energistics Identifier Specification v5.0, section 2.3.1 (p. 9).
    #[test]
    fn the_nil_uuid_is_only_all_zeros() {
        assert!(is_nil_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(is_nil_uuid(" 00000000-0000-0000-0000-000000000000\n"));
        assert!(!is_nil_uuid("00000000-0000-0000-0000-000000000001"));
        assert!(!is_nil_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(!is_nil_uuid("0"));
        assert!(!is_nil_uuid(""));
    }

    #[test]
    fn content_types() {
        let ct =
            ContentType::parse("application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation")
                .unwrap();
        assert_eq!(ct.domain, "resqml");
        assert_eq!(ct.version, "2.2");
        assert_eq!(ct.object_type.as_deref(), Some("TriangulatedSetRepresentation"));

        // RESQML 2.0.1 spells top-level objects with the obj_ prefix, and it
        // must survive: fesapi writes it, and the EPC part is named after it.
        let raw = "application/x-resqml+xml;version=2.0;type=obj_TriangulatedSetRepresentation";
        let old = ContentType::parse(raw).unwrap();
        assert_eq!(
            old.object_type.as_deref(),
            Some("obj_TriangulatedSetRepresentation")
        );
        assert_eq!(
            old.to_string(),
            raw,
            "a parsed content type must round trip verbatim"
        );
        assert_eq!(old.type_key(), Some("TriangulatedSetRepresentation"));

        // A referenced non-EnergyML part.
        let h5 = ContentType::parse("application/x-hdf5;version=1.0").unwrap();
        assert_eq!(h5.raw_mime.as_deref(), Some("x-hdf5"));
    }

    #[test]
    fn qualified_types() {
        let qt = QualifiedType::parse("resqml22.TriangulatedSetRepresentation").unwrap();
        assert_eq!(qt.domain, "resqml");
        assert_eq!(qt.version, "22");
        assert_eq!(qt.dotted_version(), "2.2");
        assert_eq!(qt.to_string(), "resqml22.TriangulatedSetRepresentation");
    }

    #[test]
    fn etp_uris() {
        let uri = EtpUri::parse(
            "eml:///dataspace('demo/ds')/resqml22.TriangulatedSetRepresentation(a1b2c3d4-e5f6-7890-abcd-ef1234567890)",
        )
        .unwrap();
        assert_eq!(uri.dataspace.as_deref(), Some("demo/ds"));
        assert_eq!(uri.object_type.as_deref(), Some("TriangulatedSetRepresentation"));
        assert!(!uri.is_dataspace());

        let ds = EtpUri::parse("eml:///dataspace('demo')").unwrap();
        assert!(ds.is_dataspace());

        let no_ds = EtpUri::parse("eml:///witsml21.Well(a1b2c3d4-e5f6-7890-abcd-ef1234567890)").unwrap();
        assert_eq!(no_ds.dataspace, None);
        assert_eq!(no_ds.domain.as_deref(), Some("witsml"));

        // Round trip - against the string read, not against itself.
        assert_eq!(
            uri.to_string(),
            "eml:///dataspace('demo/ds')/resqml22.TriangulatedSetRepresentation(a1b2c3d4-e5f6-7890-abcd-ef1234567890)"
        );
    }

    /// The version of an object is part of a canonical URI (ETP v1.2
    /// §25.3.7, §25.3.10): read, kept apart from the schema version, and
    /// written back.
    #[test]
    fn a_versioned_object_uri_reads_and_writes_back() {
        let text = "eml:///dataspace('rdms-db')/resqml20.obj_HorizonInterpretation(uuid=421a7a05-033a-450d-bcef-051352023578,version='2.0')";
        let uri = EtpUri::parse(text).unwrap();
        assert_eq!(uri.uuid.as_deref(), Some("421a7a05-033a-450d-bcef-051352023578"));
        assert_eq!(uri.object_version.as_deref(), Some("2.0"));
        assert_eq!(
            uri.version.as_deref(),
            Some("20"),
            "the schema version is another field"
        );
        assert_eq!(uri.rest, None);
        assert_eq!(uri.to_string(), text);
        assert!(EtpUri::is_canonical(text));
        // `uuid=` without a version reads, and is not canonical (§25.3.9 item 7).
        let alternate =
            "eml:///resqml20.obj_HorizonInterpretation(uuid=421a7a05-033a-450d-bcef-051352023578)";
        assert!(EtpUri::parse(alternate).is_ok());
        assert!(!EtpUri::is_canonical(alternate));
    }
}
