//! Object identity, independent of the schema version it was read from.
//!
//! A top-level object and a `DataObjectReference` say the same thing in
//! different words, and each version of eml words it differently again:
//!
//! | | eml 2.0 | eml 2.1 / 2.2 | eml 2.3 |
//! |---|---|---|---|
//! | identifier | `UUID` | `Uuid` | `Uuid` |
//! | target type | `ContentType` | `ContentType` | `QualifiedType` |
//! | object version | `VersionString` | `VersionString` | `ObjectVersion` |
//!
//! [`ObjectKey`] is the pivot they all reduce to. It **borrows**: from a
//! top-level object it borrows the `&'static str`s of the type's [`TypeInfo`]
//! and the uuid out of the instance; from a reference it borrows sub-slices of
//! the `ContentType` or `QualifiedType` string the reference already owns.
//! Comparing, indexing and walking a reference graph therefore allocate
//! nothing. [`ObjectId`] is the owned twin, for storing and serializing.
//!
//! Creating a reference in any version, from anything identifiable in any
//! version, is one call:
//!
//! ```ignore
//! let dor: eml23::DataObjectReference = well.as_object_ref()?;
//! let dor_20 = eml20::DataObjectReference::referencing(&well)?;
//! ```
//!
//! Names are handled per [`crate::naming`]: written verbatim, compared exactly,
//! and compared loosely only where the caller asks for it.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::naming;
use crate::reflect::TypeInfo;
use crate::value::{FieldValue, OwnedValue, Reflect, Scalar};

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

/// A schema version, in whichever form its source wrote it.
///
/// Content types spell it `2.0`, qualified types spell it `20`. Keeping the
/// borrowed form means a key built from a reference allocates nothing; the
/// conversion happens only when the other spelling is actually asked for.
#[derive(Clone, Copy, Debug)]
pub enum VersionRef<'a> {
    /// As a content type writes it: `2.0`.
    Dotted(&'a str),
    /// As a qualified type writes it: `20`.
    Compact(&'a str),
}

impl<'a> VersionRef<'a> {
    /// Dotted form, borrowed when it already is one.
    pub fn dotted(&self) -> Cow<'a, str> {
        match self {
            VersionRef::Dotted(v) => Cow::Borrowed(v),
            VersionRef::Compact(v) => Cow::Owned(naming::dotted_version(v)),
        }
    }

    /// Compact form, borrowed when it already is one.
    pub fn compact(&self) -> Cow<'a, str> {
        match self {
            VersionRef::Compact(v) => Cow::Borrowed(v),
            VersionRef::Dotted(v) => Cow::Owned(naming::compact_version(v)),
        }
    }

    /// Same schema version, whichever spelling each side uses.
    pub fn same_as(&self, other: &VersionRef<'_>) -> bool {
        self.compact() == other.compact()
    }
}

// ---------------------------------------------------------------------------
// The pivot
// ---------------------------------------------------------------------------

/// How an EPC lays its parts out. Both are permitted by the packaging
/// conventions, and a reader must accept either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpcLayout {
    /// Every XML part at the root of the archive.
    Flat,
    /// One folder per package, e.g. `resqml20/`.
    PerPackage,
}

/// Identity of an EnergyML object, borrowed from wherever it was found.
///
/// `object_type` is **verbatim**: `obj_Activity` keeps its prefix, because that
/// is what goes into a content type, a qualified type and an EPC part name.
/// Use [`ObjectKey::type_key`] to group across versions.
#[derive(Clone, Copy, Debug)]
pub struct ObjectKey<'a> {
    /// `eml`, `resqml`, `witsml` or `prodml`.
    pub domain: &'a str,
    /// Schema version, in the form its source wrote.
    pub version: VersionRef<'a>,
    /// Complex type name, verbatim - `obj_Activity`, `TriangulatedSetRepresentation`.
    pub object_type: &'a str,
    /// UUID exactly as read; never rewritten, only compared without case.
    pub uuid: &'a str,
    /// `objectVersion` / `VersionString`. Absent means "the latest".
    pub object_version: Option<&'a str>,
    /// Title. In a reference this is a hint, not a source of truth.
    pub title: Option<&'a str>,
}

impl<'a> ObjectKey<'a> {
    /// Canonical grouping key: the type name without `obj_`, case preserved.
    /// The same value for `obj_Activity` (2.0.1) and `Activity` (2.2), which is
    /// what a version translator keys its rules on.
    pub fn type_key(&self) -> &'a str {
        naming::strip_obj_prefix(self.object_type)
    }

    /// `application/x-resqml+xml;version=2.0;type=obj_Activity`
    pub fn content_type(&self) -> String {
        format!(
            "application/x-{}+xml;version={};type={}",
            self.domain,
            self.version.dotted(),
            self.object_type
        )
    }

    /// `resqml20.obj_Activity`
    pub fn qualified_type(&self) -> String {
        format!("{}{}.{}", self.domain, self.version.compact(), self.object_type)
    }

    /// **The** uri of an object: `eml:///dataspace('demo')/resqml20.obj_Activity(uuid)`,
    /// without the dataspace segment when none is given - and with the object's
    /// version whenever it declares one, `…(uuid=…,version='2')` (Identifier
    /// Specification v5.0 §3.4.2, p. 14). Internally the uri always carries the
    /// version number when the object has one (ADR-0003). An empty or blank
    /// version is no version.
    ///
    /// Built from an object read through reflection by [`key_of_reflect`] (the
    /// uuid by its role, the type by the object's own type or the reference's
    /// `ContentType`/`QualifiedType`, the version by `objectVersion` or
    /// `Citation.VersionString`), this is the one constructor of an identity.
    pub fn etp_uri(&self, dataspace: Option<&str>) -> String {
        crate::identifiers::EtpUri::object_versioned(
            dataspace,
            &self.qualified_type(),
            self.uuid,
            self.object_version
                .map(str::trim)
                .filter(|version| !version.is_empty()),
        )
    }

    /// EPC part name, without any folder: `obj_Activity_<uuid>.xml`.
    pub fn part_name(&self) -> String {
        format!("{}_{}.xml", self.object_type, self.uuid)
    }

    /// EPC part path for one of the two permitted layouts.
    pub fn part_path(&self, layout: EpcLayout) -> String {
        match layout {
            EpcLayout::Flat => self.part_name(),
            EpcLayout::PerPackage => {
                format!("{}{}/{}", self.domain, self.version.compact(), self.part_name())
            }
        }
    }

    /// Same object: same uuid (case-insensitively), and object versions that do
    /// not contradict each other - an absent one means "the latest", so it
    /// matches anything.
    pub fn same_object(&self, other: &ObjectKey<'_>) -> bool {
        self.uuid.eq_ignore_ascii_case(other.uuid)
            && versions_agree(self.object_version, other.object_version)
    }

    /// Same concept, whatever the version: `obj_Activity` of resqml 2.0.1 and
    /// `Activity` of resqml 2.2 match.
    pub fn same_concept(&self, other: &ObjectKey<'_>) -> bool {
        self.domain.eq_ignore_ascii_case(other.domain)
            && naming::type_eq_loose(self.object_type, other.object_type)
    }

    /// Copy into an owned identity.
    pub fn to_owned_id(&self) -> ObjectId {
        ObjectId {
            domain: self.domain.to_string(),
            version: self.version.dotted().into_owned(),
            object_type: self.object_type.to_string(),
            uuid: self.uuid.to_string(),
            object_version: self.object_version.map(str::to_string),
            title: self.title.map(str::to_string),
        }
    }
}

/// Identity of an EnergyML object, owned - for storing, hashing and
/// serializing. `version` is always the dotted form.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectId {
    /// `eml`, `resqml`, `witsml` or `prodml`.
    pub domain: String,
    /// Schema version, dotted: `2.0`.
    pub version: String,
    /// Complex type name, verbatim, `obj_` included where the schema has it.
    pub object_type: String,
    /// UUID as read.
    pub uuid: String,
    /// `objectVersion`, absent meaning "the latest".
    pub object_version: Option<String>,
    /// Title, indicative.
    pub title: Option<String>,
}

impl ObjectId {
    /// Borrowed view, so every [`ObjectKey`] method is available here too.
    pub fn as_key(&self) -> ObjectKey<'_> {
        ObjectKey {
            domain: &self.domain,
            version: VersionRef::Dotted(&self.version),
            object_type: &self.object_type,
            uuid: &self.uuid,
            object_version: self.object_version.as_deref(),
            title: self.title.as_deref(),
        }
    }

    /// Grouping key: type name without `obj_`.
    pub fn type_key(&self) -> &str {
        naming::strip_obj_prefix(&self.object_type)
    }

    /// `application/x-resqml+xml;version=2.0;type=obj_Activity`
    pub fn content_type(&self) -> String {
        self.as_key().content_type()
    }

    /// `resqml20.obj_Activity`
    pub fn qualified_type(&self) -> String {
        self.as_key().qualified_type()
    }

    /// ETP 1.2 URI, with or without a dataspace.
    pub fn etp_uri(&self, dataspace: Option<&str>) -> String {
        self.as_key().etp_uri(dataspace)
    }

    /// Build from a content type plus a uuid.
    pub fn from_content_type(content_type: &str, uuid: &str) -> Result<Self> {
        let parsed = parse_content_type(content_type)?;
        Ok(ObjectId {
            domain: parsed.domain.to_string(),
            version: parsed.version.to_string(),
            object_type: parsed.object_type.to_string(),
            uuid: uuid.trim().to_string(),
            object_version: None,
            title: None,
        })
    }

    /// Build from a qualified type plus a uuid.
    pub fn from_qualified_type(qualified_type: &str, uuid: &str) -> Result<Self> {
        let parsed = parse_qualified_type(qualified_type)?;
        Ok(ObjectId {
            domain: parsed.domain.to_string(),
            version: naming::dotted_version(parsed.version),
            object_type: parsed.object_type.to_string(),
            uuid: uuid.trim().to_string(),
            object_version: None,
            title: None,
        })
    }

    /// Build from an ETP URI, returning the dataspace it carried, if any.
    ///
    /// The uri is read by [`EtpUri::parse`](crate::identifiers::EtpUri::parse),
    /// the one door, and the identity is built from what it returns: this keeps
    /// no reading of its own. So what the door refuses this refuses - a uuid
    /// that is not a uuid, a type with a character a type cannot carry - and the
    /// uuid comes back in lowercase. Two things the door reads and an identity
    /// cannot hold are refused here, by name: a uri that names a dataspace and no
    /// object, and one that goes on below the object (`/resqml22.Foo?query`).
    /// The versioned form, `(uuid=…,version='…')` - Identifier Specification
    /// v5.0 §3.4.2 (p. 14) - gives the object's version: part of the identity,
    /// never of the uuid.
    pub fn from_etp_uri(uri: &str) -> Result<(Option<String>, Self)> {
        let parsed = crate::identifiers::EtpUri::parse(uri)?;
        let (Some(domain), Some(version), Some(object_type), Some(uuid)) =
            (parsed.domain, parsed.version, parsed.object_type, parsed.uuid)
        else {
            return Err(Error::parse(uri, "etpUri", "names a dataspace, not an object"));
        };
        if parsed.rest.is_some() {
            return Err(Error::parse(
                uri,
                "etpUri",
                "goes on below the object: it names a collection, not an object",
            ));
        }
        Ok((
            parsed.dataspace,
            ObjectId {
                domain,
                version: naming::dotted_version(&version),
                object_type,
                uuid,
                object_version: parsed.object_version,
                title: None,
            },
        ))
    }

    /// Identity of a type, with a uuid supplied separately. `None` for a type
    /// that is not a top-level object.
    pub fn of_type(info: &'static TypeInfo, uuid: &str) -> Option<Self> {
        if !info.is_top_level {
            return None;
        }
        Some(ObjectId {
            domain: info.package.to_string(),
            version: info.schema_version.to_string(),
            object_type: info.object_type.to_string(),
            uuid: uuid.trim().to_string(),
            object_version: None,
            title: None,
        })
    }

    /// **The identity of an object**, read from the object itself through
    /// reflection - a top-level object or a reference to one (a
    /// `DataObjectReference`, a `ContactElementReference`, a `ContactElement`):
    /// the uuid by its role (`/^u?uid$/i`, resolved when the code was
    /// generated), the type by the object's own type or the reference's
    /// `ContentType`/`QualifiedType`, the version by `objectVersion` or
    /// `Citation.VersionString`. `None` when the object names no identity.
    /// With [`ObjectId::etp_uri`], the one constructor of a uri.
    pub fn of_reflect(object: &dyn Reflect) -> Option<ObjectId> {
        key_of_reflect(object).map(|key| key.to_owned_id())
    }

    /// The identity a uri names, and the dataspace it names when it names one -
    /// the one reading of a text into an identity ([`ObjectId::from_etp_uri`]).
    pub fn parse(uri: &str) -> Result<(Option<String>, ObjectId)> {
        Self::from_etp_uri(uri)
    }

    /// What a view shows of an object: its type, its title (its uuid when it
    /// has none), and its version **when it declares one**: any view that shows
    /// an object's type and title also shows its version unless that is empty
    /// or absent (ADR-0003).
    /// `resqml22.TriangulatedSetRepresentation - H3 (version 2)`.
    pub fn label(&self, title: Option<&str>) -> String {
        let title = title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&self.uuid);
        match self
            .object_version
            .as_deref()
            .map(str::trim)
            .filter(|version| !version.is_empty())
        {
            Some(version) => format!("{} - {title} (version {version})", self.qualified_type()),
            None => format!("{} - {title}", self.qualified_type()),
        }
    }

    /// Same object, per [`ObjectKey::same_object`].
    pub fn same_object(&self, other: &ObjectId) -> bool {
        self.as_key().same_object(&other.as_key())
    }
}

impl PartialEq for ObjectId {
    /// Uuid is compared without case - hexadecimal is case-insensitive, and
    /// writers disagree about it - everything else exactly.
    fn eq(&self, other: &Self) -> bool {
        self.uuid.eq_ignore_ascii_case(&other.uuid)
            && self.domain == other.domain
            && self.version == other.version
            && self.object_type == other.object_type
            && self.object_version == other.object_version
    }
}

impl Eq for ObjectId {}

impl std::hash::Hash for ObjectId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uuid.to_ascii_lowercase().hash(state);
        self.domain.hash(state);
        self.version.hash(state);
        self.object_type.hash(state);
        self.object_version.hash(state);
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.qualified_type(), self.uuid)
    }
}

// ---------------------------------------------------------------------------
// Parsing, without regex: every piece is a sub-slice of the input
// ---------------------------------------------------------------------------

/// The three parts of a content type, borrowed.
pub struct ContentTypeParts<'a> {
    /// `resqml`
    pub domain: &'a str,
    /// `2.0`, dotted
    pub version: &'a str,
    /// `obj_Activity`, verbatim
    pub object_type: &'a str,
}

/// What a content type says, borrowed: the one reading of the grammar
/// `application/x-<domain>+xml;version=<v>[;type=<T>]` and of the media types
/// that are not Energistics ones (`application/x-hdf5;version=1.0`).
pub(crate) struct ContentTypeSplit<'a> {
    /// `resqml` - `None` for a media type that is not `x-<domain>+xml`.
    pub(crate) domain: Option<&'a str>,
    /// The media type after `application/`, when it is not `x-<domain>+xml`.
    pub(crate) mime: Option<&'a str>,
    /// `2.0`, as written.
    pub(crate) version: &'a str,
    /// `obj_Activity`, verbatim - `None` for a part that is not an object.
    pub(crate) object_type: Option<&'a str>,
}

fn is_word(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Read a content type, tolerating the order of `version=` and `type=` and
/// ignoring any other parameter. [`ContentType::parse`](crate::identifiers::ContentType::parse)
/// and [`parse_content_type`] both read through it.
pub(crate) fn split_content_type(value: &str) -> Result<ContentTypeSplit<'_>> {
    let err = || {
        Error::parse(
            value,
            "contentType",
            "expected application/x-<domain>+xml;version=..;type=..",
        )
    };
    let mut parts = value.split(';').map(str::trim);
    let media = parts
        .next()
        .and_then(|media| media.strip_prefix("application/"))
        .ok_or_else(err)?;
    let (domain, mime) = match media
        .strip_prefix("x-")
        .and_then(|rest| rest.strip_suffix("+xml"))
    {
        Some(domain) if is_word(domain) => (Some(domain), None),
        _ if !media.is_empty()
            && media
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '+' | '-')) =>
        {
            (None, Some(media))
        }
        _ => return Err(err()),
    };

    let mut version = None;
    let mut object_type = None;
    for part in parts {
        if let Some(v) = part.strip_prefix("version=") {
            let v = v.trim();
            if v.is_empty() || v.chars().any(char::is_whitespace) {
                return Err(err());
            }
            version = Some(v);
        } else if let Some(t) = part.strip_prefix("type=") {
            let t = t.trim();
            if !is_word(t) {
                return Err(err());
            }
            object_type = Some(t);
        }
    }

    Ok(ContentTypeSplit {
        domain,
        mime,
        version: version.ok_or_else(err)?,
        object_type,
    })
}

/// Split `application/x-resqml+xml;version=2.0;type=obj_Activity`.
///
/// The `obj_` prefix is **kept**: stripping it here is how a 2.0.1 content type
/// gets corrupted on the way back out. A content type that names no domain or no
/// type is refused.
pub fn parse_content_type(value: &str) -> Result<ContentTypeParts<'_>> {
    let split = split_content_type(value)?;
    match (split.domain, split.object_type) {
        (Some(domain), Some(object_type)) => Ok(ContentTypeParts {
            domain,
            version: split.version,
            object_type,
        }),
        _ => Err(Error::parse(
            value,
            "contentType",
            "expected application/x-<domain>+xml;version=..;type=..",
        )),
    }
}

/// The three parts of a qualified type, borrowed.
pub struct QualifiedTypeParts<'a> {
    /// `resqml`
    pub domain: &'a str,
    /// `20`, compact
    pub version: &'a str,
    /// `obj_Activity`, verbatim
    pub object_type: &'a str,
}

/// Split `resqml20.obj_Activity`, keeping the `obj_` prefix.
pub fn parse_qualified_type(value: &str) -> Result<QualifiedTypeParts<'_>> {
    let err = || Error::parse(value, "qualifiedType", "expected <domain><version>.<Type>");
    let value = value.trim();
    let (head, object_type) = value.split_once('.').ok_or_else(err)?;
    let split = head.find(|c: char| c.is_ascii_digit()).ok_or_else(err)?;
    let (domain, version) = head.split_at(split);
    if domain.is_empty() || !domain.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(err());
    }
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        return Err(err());
    }
    if !is_word(object_type) {
        return Err(err());
    }
    Ok(QualifiedTypeParts {
        domain,
        version,
        object_type,
    })
}

// ---------------------------------------------------------------------------
// The traits the derive implements
// ---------------------------------------------------------------------------

/// What kind of identity a type carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityKind {
    /// A top-level object: it *is* the thing.
    TopLevel,
    /// A reference to one: `DataObjectReference` and the types extending it.
    ObjectRef,
    /// Neither - an ordinary nested type.
    None,
}

/// How a reference names its target. Which one a version uses is the whole
/// reason this abstraction exists.
#[derive(Clone, Copy, Debug)]
pub enum DeclaredType<'a> {
    /// eml 2.0 / 2.1 / 2.2 write a content type.
    ContentType(&'a str),
    /// eml 2.3 writes a qualified type.
    QualifiedType(&'a str),
}

/// Identity, readable. Implemented by the derive for every top-level object and
/// every reference type; object-safe, so it works behind `&dyn`.
pub trait ObjectIdentity {
    /// Top-level, reference, or neither.
    fn identity_kind(&self) -> IdentityKind;

    /// Static description of the concrete type.
    fn identity_type(&self) -> &'static TypeInfo;

    /// UUID as stored.
    fn uuid(&self) -> Option<&str>;

    /// `Citation.Title` for a top-level object, `Title` for a reference.
    fn title(&self) -> Option<&str>;

    /// `objectVersion` / `VersionString`.
    fn object_version(&self) -> Option<&str>;

    /// The type string a reference carries. `None` for a top-level object,
    /// whose type is known statically.
    fn declared_type(&self) -> Option<DeclaredType<'_>> {
        None
    }
}

/// Identity, writable - for building objects and references dynamically.
pub trait ObjectIdentityMut: ObjectIdentity {
    /// Set the uuid, rejecting anything that is not one.
    fn set_uuid(&mut self, uuid: &str) -> Result<()>;
    /// Set the title, creating the citation if the type has one and it is absent.
    fn set_title(&mut self, title: &str) -> Result<()>;
    /// Set the object version.
    fn set_object_version(&mut self, version: Option<&str>) -> Result<()>;
    /// Point a reference at a target, writing the form this version uses.
    fn set_target(&mut self, key: &ObjectKey<'_>) -> Result<()>;
}

/// A reference type: `DataObjectReference` in any eml version, and everything
/// extending it.
pub trait ObjectRef: ObjectIdentity + Default + Sized {
    /// Fill in the fields this version knows about, and only those.
    fn from_key(key: &ObjectKey<'_>) -> Self;

    /// Reference anything identifiable - a top-level object of any version, or
    /// another reference of any version.
    fn referencing<T: ObjectIdentity + ?Sized>(source: &T) -> Result<Self> {
        let key = key_of(source)
            .ok_or_else(|| Error::other(format!("`{}` carries no identity", source.identity_type().name)))?;
        Ok(Self::from_key(&key))
    }
}

/// Ergonomic side of [`ObjectRef`], available on anything identifiable.
pub trait AsObjectRef: ObjectIdentity {
    /// `let dor: eml23::DataObjectReference = well.as_object_ref()?;`
    fn as_object_ref<D: ObjectRef>(&self) -> Result<D>;
}

impl<T: ObjectIdentity + ?Sized> AsObjectRef for T {
    fn as_object_ref<D: ObjectRef>(&self) -> Result<D> {
        D::referencing(self)
    }
}

// ---------------------------------------------------------------------------
// Building a key - the generic part stays thin on purpose
// ---------------------------------------------------------------------------

/// Identity of anything identifiable, borrowed.
///
/// The generic wrapper only forwards six already-computed values, so the real
/// work below is compiled once rather than for each of the thousand generated
/// types.
pub fn key_of<'a, T: ObjectIdentity + ?Sized>(source: &'a T) -> Option<ObjectKey<'a>> {
    key_from_parts(
        source.identity_kind(),
        source.identity_type(),
        source.uuid(),
        source.title(),
        source.object_version(),
        source.declared_type(),
    )
}

/// The non-generic core of [`key_of`].
pub fn key_from_parts<'a>(
    kind: IdentityKind,
    info: &'static TypeInfo,
    uuid: Option<&'a str>,
    title: Option<&'a str>,
    object_version: Option<&'a str>,
    declared: Option<DeclaredType<'a>>,
) -> Option<ObjectKey<'a>> {
    let uuid = uuid?;
    match kind {
        IdentityKind::None => None,
        IdentityKind::TopLevel => Some(ObjectKey {
            domain: info.package,
            version: VersionRef::Dotted(info.schema_version),
            object_type: info.object_type,
            uuid,
            object_version,
            title,
        }),
        IdentityKind::ObjectRef => match declared? {
            DeclaredType::ContentType(ct) => {
                let parts = parse_content_type(ct).ok()?;
                Some(ObjectKey {
                    domain: parts.domain,
                    version: VersionRef::Dotted(parts.version),
                    object_type: parts.object_type,
                    uuid,
                    object_version,
                    title,
                })
            }
            DeclaredType::QualifiedType(qt) => {
                let parts = parse_qualified_type(qt).ok()?;
                Some(ObjectKey {
                    domain: parts.domain,
                    version: VersionRef::Compact(parts.version),
                    object_type: parts.object_type,
                    uuid,
                    object_version,
                    title,
                })
            }
        },
    }
}

/// Content type of anything identifiable. Borrowed whenever the string already
/// exists - a top-level object's `TypeInfo`, or a reference that already stores
/// a content type - and built only when a conversion is unavoidable.
pub fn content_type_of<'a, T: ObjectIdentity + ?Sized>(source: &'a T) -> Option<Cow<'a, str>> {
    if let Some(DeclaredType::ContentType(ct)) = source.declared_type() {
        return Some(Cow::Borrowed(ct));
    }
    if source.identity_kind() == IdentityKind::TopLevel
        && let Some(ct) = source.identity_type().content_type
    {
        return Some(Cow::Borrowed(ct));
    }
    key_of(source).map(|k| Cow::Owned(k.content_type()))
}

/// Qualified type of anything identifiable, borrowed where possible.
pub fn qualified_type_of<'a, T: ObjectIdentity + ?Sized>(source: &'a T) -> Option<Cow<'a, str>> {
    if let Some(DeclaredType::QualifiedType(qt)) = source.declared_type() {
        return Some(Cow::Borrowed(qt));
    }
    if source.identity_kind() == IdentityKind::TopLevel
        && let Some(qt) = source.identity_type().qualified_type
    {
        return Some(Cow::Borrowed(qt));
    }
    key_of(source).map(|k| Cow::Owned(k.qualified_type()))
}

/// ETP URI of anything identifiable.
pub fn etp_uri_of<T: ObjectIdentity + ?Sized>(source: &T, dataspace: Option<&str>) -> Option<String> {
    key_of(source).map(|k| k.etp_uri(dataspace))
}

// ---------------------------------------------------------------------------
// The dynamic path: same fields, reached through reflection
// ---------------------------------------------------------------------------

/// Read a role-carrying field of an object reached through reflection.
///
/// The role indices live in [`TypeInfo::roles`] and are resolved at generation
/// time, so this is an index into a static array - not a search by name.
fn role_str(object: &dyn Reflect, index: Option<u16>) -> Option<&str> {
    let info = object.type_info();
    let field = info.fields.get(index? as usize)?;
    match object.field(field.name)? {
        FieldValue::Scalar(Scalar::Str(s)) => Some(s),
        _ => None,
    }
}

/// UUID of an object reached dynamically.
pub fn uuid_of_reflect(object: &dyn Reflect) -> Option<&str> {
    role_str(object, object.type_info().roles.uuid)
}

/// The uuid of an object or a reference reached dynamically, as stored, when
/// it carries a non-empty one: the reading every reader of a reference's uuid
/// shares (an empty uuid names nothing).
pub fn non_empty_uuid_of_reflect(object: &dyn Reflect) -> Option<String> {
    uuid_of_reflect(object)
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

/// The uuid of the object a field of `object` references, as stored - `None`
/// when the field is absent, is no reference, or names no uuid.
pub fn reference_uuid(object: &dyn Reflect, field: &str) -> Option<String> {
    non_empty_uuid_of_reflect(object.field(field)?.as_object()?)
}

/// Title of an object reached dynamically: its own `Title`, or the one inside
/// its `Citation`.
pub fn title_of_reflect(object: &dyn Reflect) -> Option<&str> {
    let roles = object.type_info().roles;
    if let Some(title) = role_str(object, roles.title) {
        return Some(title);
    }
    let citation = object.field(object.type_info().fields.get(roles.citation? as usize)?.name)?;
    let citation = citation.as_object()?;
    role_str(citation, citation.type_info().roles.title)
}

/// Object version of an object reached dynamically: its own `objectVersion`,
/// or the `VersionString` inside its `Citation` - where eml 2.0 keeps it.
///
/// The second half was missing until 10/09, and it read as a real finding:
/// `energyml check` called a reference *at version `my version`* to an
/// object that *carries no objectVersion*, on a 2.0.1 package whose object
/// says `my version` in its citation. The same shape as `title_of_reflect`.
pub fn object_version_of_reflect(object: &dyn Reflect) -> Option<&str> {
    let roles = object.type_info().roles;
    if let Some(version) = role_str(object, roles.object_version) {
        return Some(version);
    }
    let citation = object.field(object.type_info().fields.get(roles.citation? as usize)?.name)?;
    let citation = citation.as_object()?;
    role_str(citation, citation.type_info().roles.object_version)
}

/// The type a reference declares, reached dynamically.
pub fn declared_type_of_reflect(object: &dyn Reflect) -> Option<DeclaredType<'_>> {
    let roles = object.type_info().roles;
    if let Some(qt) = role_str(object, roles.qualified_type) {
        return Some(DeclaredType::QualifiedType(qt));
    }
    role_str(object, roles.content_type).map(DeclaredType::ContentType)
}

/// Identity of an object reached dynamically - works on a `Box<dyn DynObject>`
/// read from an EPC without knowing its concrete type.
pub fn key_of_reflect(object: &dyn Reflect) -> Option<ObjectKey<'_>> {
    let info = object.type_info();
    key_from_parts(
        identity_kind_of(info),
        info,
        uuid_of_reflect(object),
        title_of_reflect(object),
        object_version_of_reflect(object),
        declared_type_of_reflect(object),
    )
}

/// **The uri of an object or of a reference to one**, reached dynamically -
/// the full identity, version included when one is declared. The one way a
/// reference becomes an address inside the library: following a reference by
/// its uuid alone reads the wrong version as soon as a package holds two.
pub fn uri_of_reflect(object: &dyn Reflect) -> Option<String> {
    key_of_reflect(object).map(|key| key.etp_uri(None))
}

/// **The address a reference is followed by**: its full uri
/// ([`uri_of_reflect`]), or - the one exception, and it is the reference's -
/// its uuid when it names **no type** (no `QualifiedType`, no `ContentType`),
/// since a uri is built from the type. A store resolves that uuid when it is
/// unique and refuses it by name when it is not. `None` for no identity at all.
pub fn address_of_reflect(object: &dyn Reflect) -> Option<String> {
    uri_of_reflect(object).or_else(|| {
        uuid_of_reflect(object)
            .map(str::trim)
            .filter(|uuid| !uuid.is_empty())
            .map(str::to_string)
    })
}

/// How the version of a held object stands against the version a reference (or
/// an address) names - **the one rule**, written here once.
///
/// *A named version is read; a short reference is accepted when nothing
/// contradicts it.* Two versions named and different are two objects; a
/// reference that names none, or an object that declares none, contradicts
/// nothing. Blank is none (a `VersionString` of spaces says nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionFit {
    /// The same version - or neither side names one. The best answer.
    Exact,
    /// One side names none: nothing contradicts. Accepted, after an exact one.
    Compatible,
    /// Both name one, and they differ: not the same object.
    Contradicts,
}

/// [`VersionFit`] of `held` against `wanted`.
pub fn version_fit(wanted: Option<&str>, held: Option<&str>) -> VersionFit {
    fn named(version: Option<&str>) -> Option<&str> {
        version.map(str::trim).filter(|version| !version.is_empty())
    }
    match (named(wanted), named(held)) {
        (Some(wanted), Some(held)) if wanted == held => VersionFit::Exact,
        (Some(_), Some(_)) => VersionFit::Contradicts,
        (None, None) => VersionFit::Exact,
        _ => VersionFit::Compatible,
    }
}

/// Whether `held` answers `wanted`: [`version_fit`] is not a contradiction.
pub fn versions_agree(wanted: Option<&str>, held: Option<&str>) -> bool {
    version_fit(wanted, held) != VersionFit::Contradicts
}

/// **The address of an object known by the parts a text gives of it** - a
/// declared type (a `ContentType` or a `QualifiedType`, whichever the text
/// holds), a uuid and an object version: the same address
/// [`address_of_reflect`] makes of a typed reference, for a reader that scans
/// text (a subtree kept as `AnyXml`). Its uri when a type is declared and
/// parses; the uuid, trimmed, when none is - the one exception, and it is the
/// reference's.
pub fn address_of_parts(declared_type: Option<&str>, uuid: &str, object_version: Option<&str>) -> String {
    let uuid = uuid.trim();
    let parsed = declared_type
        .map(str::trim)
        .filter(|declared| !declared.is_empty())
        .and_then(|declared| {
            if declared.contains("+xml") {
                ObjectId::from_content_type(declared, uuid).ok()
            } else {
                ObjectId::from_qualified_type(declared, uuid).ok()
            }
        });
    match parsed {
        Some(mut id) => {
            id.object_version = object_version
                .map(str::trim)
                .filter(|version| !version.is_empty())
                .map(str::to_string);
            id.etp_uri(None)
        }
        None => uuid.to_string(),
    }
}

/// **What an address names**: its uuid (lowercase) and the object version it
/// names, when it names one. An address is a uri - the identity - or, at the
/// library's entry only, a bare uuid, which names no version. The one reading
/// of an address into the two things a comparison needs.
pub fn parse_address(address: &str) -> (String, Option<String>) {
    let address = address.trim();
    if address
        .get(..4)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("eml:"))
        && let Ok(parsed) = crate::identifiers::EtpUri::parse(address)
        && let Some(uuid) = parsed.uuid
    {
        return (uuid.to_ascii_lowercase(), parsed.object_version);
    }
    (address.to_ascii_lowercase(), None)
}

/// Whether an address names an object at all: a uri with a uuid, a bare uuid -
/// or any other text the reader of an address keeps as it stands - but not an
/// empty text and not a uri that names a dataspace and no object.
fn names_an_object(address: &str) -> bool {
    let address = address.trim();
    if address.is_empty() {
        return false;
    }
    match crate::identifiers::EtpUri::parse(address) {
        Ok(parsed) if address.get(..4).is_some_and(|s| s.eq_ignore_ascii_case("eml:")) => {
            parsed.uuid.is_some()
        }
        _ => true,
    }
}

/// **Whether two addresses name the same object**: the same uuid (read by
/// [`parse_address`]), and object versions that do not contradict each other -
/// an absent version means "the latest", so it matches any
/// ([`ObjectKey::same_object`]). The one comparison of two addresses: the
/// libraries above this one call it and keep none of their own.
///
/// An address that names no object - an empty text, a uri of a dataspace -
/// is the same as nothing, **not even as itself**: two dataspace uris carry no
/// uuid, and `None == None` is no identity.
pub fn same_address(a: &str, b: &str) -> bool {
    if !names_an_object(a) || !names_an_object(b) {
        return false;
    }
    let (uuid_a, version_a) = parse_address(a);
    let (uuid_b, version_b) = parse_address(b);
    uuid_a == uuid_b && versions_agree(version_a.as_deref(), version_b.as_deref())
}

/// What kind of identity a type carries, from its metadata alone.
pub fn identity_kind_of(info: &'static TypeInfo) -> IdentityKind {
    if info.is_object_ref {
        IdentityKind::ObjectRef
    } else if info.is_top_level {
        IdentityKind::TopLevel
    } else {
        IdentityKind::None
    }
}

/// Write a role-carrying string field through reflection.
fn set_role_str(object: &mut dyn Reflect, index: Option<u16>, value: &str) -> Result<bool> {
    let Some(index) = index else { return Ok(false) };
    let Some(field) = object.type_info().fields.get(index as usize) else {
        return Ok(false);
    };
    object.set_field(field.name, OwnedValue::Str(value.to_string()))?;
    Ok(true)
}

/// Write an object's uuid, wherever its type keeps it.
///
/// `uuid`, `UUID` or `uid` depending on the release and the type - the roles
/// resolved it at generation time, so this is an index rather than a search,
/// and a caller does not have to know which spelling it is looking at.
///
/// Returns whether anything was written: a type that carries no identity has
/// nowhere to put one, and that is an answer rather than a failure.
///
/// **The value is validated first.** A uuid written here is what the object's
/// URI is built from, so a malformed one does not produce a bad field - it
/// produces an object that cannot be a part, discovered much later. The refusal
/// names the value.
pub fn set_uuid_reflect(object: &mut dyn Reflect, uuid: &str) -> Result<bool> {
    let normalised = crate::identifiers::normalize_uuid(uuid)?;
    let index = object.type_info().roles.uuid;
    set_role_str(object, index, &normalised)
}

/// Point a reference at a target, writing whichever form this version uses.
///
/// eml 2.0 / 2.1 / 2.2 get a `ContentType`, eml 2.3 a `QualifiedType`; the
/// caller does not have to know which.
///
/// # A reference that names no type is refused
///
/// One of the two spellings is **mandatory** - it is what says *what* is being
/// pointed at, and a `DataObjectReference` carrying a uuid and nothing else
/// cannot be resolved by anything: not by this build, not by fesapi, not by an
/// ETP server, which needs the qualified type to form the URI at all.
///
/// This used to be able to happen quietly. The two writes were attempted and
/// their result **discarded**, so a reference type whose role table named no
/// type field - a hand-written model, a release whose spelling the roles do not
/// yet detect - produced an object that looked built, went into the package,
/// and came back a dangling reference at the far end, where nothing points at
/// the call that made it.
///
/// So the writes are counted and none landing is an error naming the type. The
/// asymmetry with the fields below is deliberate: a title is a cache and an
/// `objectVersion` means "the latest" when absent, but a type is the reference.
pub fn set_target_reflect(object: &mut dyn Reflect, key: &ObjectKey<'_>) -> Result<()> {
    let roles = object.type_info().roles;
    let mut named = false;
    if roles.qualified_type.is_some() {
        named |= set_role_str(object, roles.qualified_type, &key.qualified_type())?;
    }
    if roles.content_type.is_some() {
        named |= set_role_str(object, roles.content_type, &key.content_type())?;
    }
    if !named {
        return Err(Error::other(format!(
            "`{}` would carry no QualifiedType and no ContentType: a reference that \
             names no type cannot be resolved by anything that reads it",
            object.type_info().name
        )));
    }
    set_role_str(object, roles.uuid, key.uuid)?;
    if let Some(title) = key.title {
        set_role_str(object, roles.title, title)?;
    }
    if let Some(version) = key.object_version {
        set_role_str(object, roles.object_version, version)?;
    }
    Ok(())
}

/// Set the title of an object reached dynamically, whether it holds a `Title`
/// directly or inside a `Citation`.
pub fn set_title_reflect(object: &mut dyn Reflect, title: &str) -> Result<()> {
    let roles = object.type_info().roles;
    if set_role_str(object, roles.title, title)? {
        return Ok(());
    }
    let Some(index) = roles.citation else {
        return Err(Error::other(format!(
            "`{}` has no title",
            object.type_info().name
        )));
    };
    let Some(field) = object.type_info().fields.get(index as usize) else {
        return Err(Error::other(format!(
            "`{}` has no title",
            object.type_info().name
        )));
    };
    let name = field.name;
    let Some(citation) = object.field_object_mut(name, None) else {
        return Err(Error::other(format!("`{name}` is absent, cannot set its title")));
    };
    let citation_roles = citation.type_info().roles;
    set_role_str(citation, citation_roles.title, title)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reference graph
// ---------------------------------------------------------------------------

/// One outgoing reference found inside an object.
#[derive(Clone, Debug)]
pub struct RefEdge<'a> {
    /// Dotted path where it was found: `Grid2dPatch.SupportingRepresentation`.
    pub path: String,
    /// What it points at.
    pub target: ObjectKey<'a>,
    /// The field's declared type, to check the target is an acceptable one.
    pub expected: Option<&'static TypeInfo>,
}

/// Every reference an object holds, found by walking reflection alone.
///
/// No schema knowledge, so a package version that does not exist yet is covered
/// the day its crate is generated. This is the raw material for `.rels`
/// regeneration, for the relationship graph and for reference validation.
///
/// Every reference is attributed to `root`, including those written inside the
/// data objects it holds by value. For the source of a relationship, which is the
/// object that writes the reference, see
/// [`identity_owned::collect_refs_owned`](crate::identity_owned::collect_refs_owned).
pub fn collect_refs(root: &dyn Reflect) -> Vec<RefEdge<'_>> {
    let mut refs = Refs { out: Vec::new() };
    walk(root, &mut refs);
    refs.out
}

/// The visitor behind [`collect_refs`]: every reference, with its full path.
struct Refs<'a> {
    out: Vec<RefEdge<'a>>,
}

impl<'a> Visitor<'a> for Refs<'a> {
    fn reference(&mut self, path: &str, child: &'a dyn Reflect, expected: Option<&'static TypeInfo>) {
        if let Some(target) = key_of_reflect(child) {
            self.out.push(RefEdge {
                path: path.to_string(),
                target,
                expected,
            });
        }
    }

    fn untyped(&mut self, path: &str, any: &'a crate::any_xml::AnyXml) {
        // The model could not type this subtree, so its references are in the
        // text. Read them rather than lose them.
        // A fault is not lost: `unreadable_subtrees` reports it by path, which
        // a visitor of this walk has no way to return.
        let _ = refs_in_source(&any.content, path, &mut self.out);
    }
}

/// A kept-verbatim subtree whose source could not be read to its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreadableSubtree {
    /// Dotted path of the subtree in the object, as for a reference.
    pub path: String,
    /// What the reader said, with the byte it stopped at.
    pub reason: String,
}

/// The kept-verbatim subtrees of `root` whose references cannot be listed
/// completely, because their source is malformed or truncated.
///
/// [`collect_refs`] returns the references it could read; this is the other half
/// of the answer, so that a reduced list is never taken for the whole one (the
/// `.rels` regenerated from it would quietly lose links). A caller that builds a
/// relationship index calls both, and names each item here as a defect.
pub fn unreadable_subtrees(root: &dyn Reflect) -> Vec<UnreadableSubtree> {
    struct Unreadable {
        out: Vec<UnreadableSubtree>,
    }
    impl<'a> Visitor<'a> for Unreadable {
        fn reference(&mut self, _: &str, _: &'a dyn Reflect, _: Option<&'static TypeInfo>) {}

        fn untyped(&mut self, path: &str, any: &'a crate::any_xml::AnyXml) {
            let mut found = Vec::new();
            if let Err(reason) = refs_in_source(&any.content, path, &mut found) {
                self.out.push(UnreadableSubtree {
                    path: path.to_string(),
                    reason,
                });
            }
        }
    }
    let mut visitor = Unreadable { out: Vec::new() };
    walk(root, &mut visitor);
    visitor.out
}

/// What a reflection walk meets.
///
/// **The one walk.** Three readers used to carry their own copy of *for each
/// field, extend the dotted path, recurse into an object, number the items of a
/// list, treat a reference and a kept-verbatim subtree apart*, and differed
/// only in what they did at a leaf. [`walk`] is that skeleton once; a reader is
/// a `Visitor` that says what to do with the three things it can meet. The path
/// is one buffer pushed and truncated as the walk goes down and up, not a
/// `format!` per field.
pub(crate) trait Visitor<'a> {
    /// A data object reference (a type with `is_object_ref`) at `path`;
    /// `expected` is the field's declared type.
    fn reference(&mut self, path: &str, child: &'a dyn Reflect, expected: Option<&'static TypeInfo>);

    /// A subtree the model could not type, kept as source text.
    fn untyped(&mut self, path: &str, any: &'a crate::any_xml::AnyXml);

    /// An ordinary object about to be entered (its fields are walked next).
    fn enter(&mut self, _path: &str, _child: &'a dyn Reflect) {}

    /// The object entered by the last unmatched [`Visitor::enter`] is done.
    fn leave(&mut self) {}
}

/// Walk `root` by reflection, reporting to `visitor`. Paths are the dotted XML
/// names from the root, `[i]` after a list item (`ChannelSet[0].Channel[2]`);
/// the root itself is not entered.
pub(crate) fn walk<'a>(root: &'a dyn Reflect, visitor: &mut impl Visitor<'a>) {
    walk_fields(root, &mut String::new(), visitor);
}

fn walk_fields<'a>(object: &'a dyn Reflect, path: &mut String, visitor: &mut impl Visitor<'a>) {
    for field in object.type_info().fields {
        let Some(value) = object.field(field.name) else {
            continue;
        };
        let mark = path.len();
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(field.xml_name);
        walk_value(value, path, field.ty.type_info(), visitor);
        path.truncate(mark);
    }
}

fn walk_value<'a>(
    value: FieldValue<'a>,
    path: &mut String,
    expected: Option<&'static TypeInfo>,
    visitor: &mut impl Visitor<'a>,
) {
    use std::fmt::Write as _;

    match value {
        FieldValue::Object(child) => {
            if child.type_info().is_object_ref {
                visitor.reference(path.as_str(), child, expected);
            } else if let Some(any) = child.as_any().downcast_ref::<crate::any_xml::AnyXml>() {
                visitor.untyped(path.as_str(), any);
            } else {
                visitor.enter(path.as_str(), child);
                walk_fields(child, path, visitor);
                visitor.leave();
            }
        }
        FieldValue::List(items) => {
            for (i, item) in items.into_iter().enumerate() {
                let mark = path.len();
                let _ = write!(path, "[{i}]");
                walk_value(item, path, expected, visitor);
                path.truncate(mark);
            }
        }
        _ => {}
    }
}

/// A data object held **inside** another one rather than as a part of its own.
#[derive(Clone, Debug)]
pub struct NestedObject<'a> {
    /// Dotted path where it sits: `ChannelSet[0].Channel[2]`.
    pub path: String,
    /// Its identity - a nested object has a uuid and a type like any other.
    pub key: ObjectKey<'a>,
}

/// Every data object nested inside `root`, at any depth - `root` itself
/// excluded.
///
/// # Why a reference may name one
///
/// A WITSML 2.1 `Log` holds its `ChannelSet`s and a `ChannelSet` its
/// `Channel`s inline, each with a uuid, each a type the schema also lets stand
/// as a part of its own; and a `ChannelSet`'s rows can only be read against
/// the channels it lists inline. PRODML's flow-test data then **references**
/// the channel set and its channels (`AbstractFlowTestData.ChannelSet`,
/// `TimeChannel`, FlowTestActivity.xsd l. 95-103) - *"Channel is a WITSML
/// object; Channels are collected into Channel Sets and the Channel Set is
/// where the actual values are stored"* (PRODML Technical Usage Guide v2.3
/// §13.6 p. 118). ETP treats the channels of a channel set as data objects of
/// their own. Such a reference **resolves to the part that holds the nested
/// object**, rather than being judged dangling.
///
/// Only a type that can be a part (`is_top_level`) and that carries a uuid
/// counts: a nested `Citation` is no data object.
pub fn nested_objects(root: &dyn Reflect) -> Vec<NestedObject<'_>> {
    let mut nested = Nested { out: Vec::new() };
    walk(root, &mut nested);
    nested.out
}

/// The visitor behind [`nested_objects`]: a data object is a type that can be a
/// part and has a non-empty uuid; a reference is not entered, and a kept-verbatim
/// subtree holds none.
struct Nested<'a> {
    out: Vec<NestedObject<'a>>,
}

impl<'a> Visitor<'a> for Nested<'a> {
    fn reference(&mut self, _path: &str, _child: &'a dyn Reflect, _expected: Option<&'static TypeInfo>) {}

    fn untyped(&mut self, _path: &str, _any: &'a crate::any_xml::AnyXml) {}

    fn enter(&mut self, path: &str, child: &'a dyn Reflect) {
        if child.type_info().is_top_level
            && let Some(key) = key_of_reflect(child)
            && !key.uuid.trim().is_empty()
        {
            self.out.push(NestedObject {
                path: path.to_string(),
                key,
            });
        }
    }
}

/// References inside a subtree the model could not type.
///
/// # Why this is not optional
///
/// An `xs:any`, or an abstract type the generator could not lower, comes back
/// as [`AnyXml`][crate::any_xml::AnyXml] - the source text, kept verbatim so
/// nothing is lost on rewrite. Nothing *of the content*, that is: the
/// references inside it were invisible to the walk above, and the walk above is
/// what regenerates a package's `.rels`.
///
/// The corpus makes the cost concrete. `GraphicalInformationSet` - the object
/// carrying every colour assignment in an EnergyML package - is exactly such a
/// case, and rewriting `testingPackageCpp22.epc` took its **18 relationships to
/// 0**. The colours survived, because the text did; the index a `.rels`-driven
/// viewer walks did not.
///
/// # What it looks for
///
/// A `DataObjectReference` is an element holding a `Uuid` and, beside it, a
/// `ContentType` or a `QualifiedType`. So this **parses** the kept source and
/// emits one edge per element that directly contains a uuid, carrying the
/// declared type when a sibling states one. A parser rather than a search for
/// angle brackets: a uuid inside a comment or an attribute value is not a
/// reference, and only the element structure can tell.
///
/// Everything borrows from `source`, which belongs to the object being walked,
/// so this allocates nothing but the paths.
///
/// # A subtree that does not read
///
/// The references found before the fault are pushed, and the fault is **returned**
/// - a malformed or truncated source, or an element never closed - never
/// swallowed into a shorter list. The visitors of the walk cannot return it, so
/// [`unreadable_subtrees`] is the door that reports it, by path.
pub(crate) fn refs_in_source<'a>(
    source: &'a str,
    prefix: &str,
    out: &mut Vec<RefEdge<'a>>,
) -> std::result::Result<(), String> {
    use quick_xml::events::Event;

    /// One open element: where it is, and what its children have said so far.
    struct Frame<'a> {
        path: String,
        uuid: Option<&'a str>,
        declared: Option<&'a str>,
        /// `ObjectVersion` (eml 2.3) or `VersionString` (eml 2.0), beside
        /// the uuid - read for the same reason the type is: a checker that
        /// saw a version on a typed reference and none on one kept as text
        /// reported every island reference as silent, whatever it said.
        object_version: Option<&'a str>,
        /// The cached `Title`, for the same reason again: unread, every
        /// reference of an island read as *untitled* and eighteen of
        /// `testingPackageCpp`'s were reported as stale copies when the text
        /// names them perfectly well.
        title: Option<&'a str>,
        seen: std::collections::HashMap<String, usize>,
    }

    let mut reader = quick_xml::Reader::from_str(source);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Frame<'a>> = Vec::new();
    let mut root_counts: std::collections::HashMap<String, usize> = Default::default();
    let mut text: Option<&'a str> = None;

    loop {
        match reader.read_event() {
            Err(why) => {
                return Err(format!(
                    "{why} (at byte {} of the kept source)",
                    reader.buffer_position()
                ));
            }
            Ok(Event::Eof) => {
                // `quick_xml` does not call an element left open at the end an
                // error; it is a truncation all the same.
                return match stack.last() {
                    Some(frame) => Err(format!(
                        "the element at `{}` is not closed: the kept source ends first",
                        frame.path
                    )),
                    None => Ok(()),
                };
            }
            Ok(Event::Start(e)) => {
                let name = local_name_of(e.name().as_ref());
                let counter = match stack.last_mut() {
                    Some(frame) => &mut frame.seen,
                    None => &mut root_counts,
                };
                let index = {
                    let n = counter.entry(name.clone()).or_insert(0);
                    let at = *n;
                    *n += 1;
                    at
                };
                let parent = stack
                    .last()
                    .map(|f| f.path.clone())
                    .unwrap_or_else(|| prefix.to_string());
                let path = match (parent.is_empty(), index) {
                    (true, 0) => name,
                    (true, n) => format!("{name}[{n}]"),
                    (false, 0) => format!("{parent}.{name}"),
                    (false, n) => format!("{parent}.{name}[{n}]"),
                };
                stack.push(Frame {
                    path,
                    uuid: None,
                    declared: None,
                    object_version: None,
                    title: None,
                    seen: Default::default(),
                });
                text = None;
            }
            // Borrowed only: a value carrying an entity would have to be
            // unescaped into a new string, and a uuid never carries one.
            Ok(Event::Text(e)) => {
                text = match e.decode() {
                    Ok(std::borrow::Cow::Borrowed(value)) => Some(value.trim()),
                    _ => None,
                };
            }
            Ok(Event::End(e)) => {
                let name = local_name_of(e.name().as_ref());
                let Some(frame) = stack.pop() else { continue };
                let value = text.take().filter(|v| !v.is_empty());
                if let (Some(parent), Some(value)) = (stack.last_mut(), value) {
                    match name.as_str() {
                        // eml 2.0 writes `UUID`, later versions `Uuid`.
                        "Uuid" | "UUID" => parent.uuid = Some(value),
                        "QualifiedType" | "ContentType" => parent.declared = Some(value),
                        "ObjectVersion" | "VersionString" => parent.object_version = Some(value),
                        "Title" => parent.title = Some(value),
                        _ => {}
                    }
                }
                let Some(uuid) = frame.uuid else { continue };
                let (domain, version, object_type) = match frame.declared {
                    Some(declared) if declared.contains("+xml") => match parse_content_type(declared) {
                        Ok(parts) => (parts.domain, VersionRef::Dotted(parts.version), parts.object_type),
                        Err(_) => ("", VersionRef::Dotted(""), ""),
                    },
                    Some(declared) => match parse_qualified_type(declared) {
                        Ok(parts) => (
                            parts.domain,
                            VersionRef::Compact(parts.version),
                            parts.object_type,
                        ),
                        Err(_) => ("", VersionRef::Dotted(""), ""),
                    },
                    None => ("", VersionRef::Dotted(""), ""),
                };
                out.push(RefEdge {
                    path: frame.path,
                    target: ObjectKey {
                        domain,
                        version,
                        object_type,
                        uuid,
                        object_version: frame.object_version,
                        title: frame.title,
                    },
                    expected: None,
                });
            }
            _ => {}
        }
    }
}

fn local_name_of(qualified: &[u8]) -> String {
    let name = String::from_utf8_lossy(qualified);
    name.rsplit(':').next().unwrap_or(&name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reference kept as text carries its version like a typed one: the
    /// eml 2.3 `ObjectVersion`, the eml 2.0 `VersionString`, and none when
    /// the text says none. Breaks if the element is read on the wrong frame
    /// or not at all - which made every island reference look silent.
    #[test]
    fn a_reference_in_kept_text_carries_its_version() {
        let source = concat!(
            r#"<eml23:TargetObject><eml23:Uuid>aaaa</eml23:Uuid><eml23:ObjectVersion>0</eml23:ObjectVersion>"#,
            r#"<eml23:QualifiedType>resqml22.BoundaryFeature</eml23:QualifiedType><eml23:Title>t</eml23:Title></eml23:TargetObject>"#,
            r#"<resqml20:Old><eml20:ContentType>application/x-resqml+xml;version=2.0;type=obj_Well</eml20:ContentType>"#,
            r#"<eml20:UUID>bbbb</eml20:UUID><eml20:VersionString>7</eml20:VersionString></resqml20:Old>"#,
            r#"<eml23:Silent><eml23:Uuid>cccc</eml23:Uuid><eml23:QualifiedType>resqml22.Fault</eml23:QualifiedType></eml23:Silent>"#,
        );
        let mut out = Vec::new();
        refs_in_source(source, "GraphicalInformation[0]", &mut out).expect("the source reads");
        let by_uuid = |uuid: &str| {
            out.iter()
                .find(|e| e.target.uuid == uuid)
                .unwrap_or_else(|| panic!("{uuid}"))
        };
        assert_eq!(out.len(), 3);
        assert_eq!(by_uuid("aaaa").target.object_version, Some("0"));
        assert_eq!(
            by_uuid("aaaa").target.title,
            Some("t"),
            "the title is there too, and reading it avoids taking it for empty"
        );
        assert_eq!(by_uuid("aaaa").path, "GraphicalInformation[0].TargetObject");
        assert_eq!(by_uuid("bbbb").target.object_version, Some("7"));
        assert_eq!(by_uuid("cccc").target.object_version, None);
    }

    /// An object holding kept-verbatim subtrees, to walk.
    #[derive(Debug, Default, Clone, PartialEq, crate::prelude::EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "Holder")]
    pub struct Holder {
        /// The subtrees.
        #[energyml(elem, xml_name = "Kept")]
        pub kept: Vec<crate::any_xml::AnyXml>,
    }

    fn subtree(content: &str) -> crate::any_xml::AnyXml {
        crate::any_xml::AnyXml {
            content: content.to_string(),
            ..Default::default()
        }
    }

    /// `[core-derive-schemas S15]`: a kept subtree whose source is malformed or
    /// truncated is **named**, with its path, rather than ending the scan
    /// silently on a shorter list. The references before the fault are kept.
    ///
    /// Break it: turn the `Err` arm of `refs_in_source` back into a `break` (or
    /// drop the open-element check at `Eof`); both assertions on the unreadable
    /// paths fail.
    #[test]
    fn a_subtree_that_does_not_read_is_named_not_swallowed() {
        let good = "<t><Uuid>aaaa</Uuid></t>";
        let holder = Holder {
            kept: vec![
                subtree(good),
                // A mismatched end tag, after one reference that reads.
                subtree("<t><Uuid>bbbb</Uuid></t><u></v>"),
                // Truncated: the element is never closed.
                subtree("<t><Uuid>cccc</Uuid></t><u><x>"),
            ],
        };

        let mut found = Vec::new();
        let outcome = refs_in_source(&holder.kept[1].content, "Kept[1]", &mut found);
        assert!(outcome.is_err(), "a mismatched end tag is a fault");
        assert_eq!(found.len(), 1, "what was read before the fault is kept");

        let unreadable = unreadable_subtrees(&holder);
        let paths: Vec<&str> = unreadable.iter().map(|item| item.path.as_str()).collect();
        assert_eq!(paths, ["Kept[1]", "Kept[2]"], "{unreadable:?}");
        assert!(
            unreadable[1].reason.contains("not closed"),
            "a truncation says so: {}",
            unreadable[1].reason
        );
        // The good one is not reported.
        assert!(!paths.contains(&"Kept[0]"));
    }

    #[test]
    fn content_type_keeps_the_obj_prefix() {
        let parts = parse_content_type("application/x-resqml+xml;version=2.0;type=obj_Activity").unwrap();
        assert_eq!(parts.domain, "resqml");
        assert_eq!(parts.version, "2.0");
        assert_eq!(parts.object_type, "obj_Activity");
    }

    #[test]
    fn content_type_round_trips_verbatim() {
        let raw = "application/x-resqml+xml;version=2.0;type=obj_Activity";
        let id = ObjectId::from_content_type(raw, "0e2ec4e1-1b0c-4b9f-8e9f-2a0d4c1f6a11").unwrap();
        assert_eq!(id.content_type(), raw);
        assert_eq!(id.qualified_type(), "resqml20.obj_Activity");
        assert_eq!(id.type_key(), "Activity");
    }

    #[test]
    fn qualified_type_round_trips_verbatim() {
        let id = ObjectId::from_qualified_type("resqml20.obj_Activity", "AAAA").unwrap();
        assert_eq!(id.version, "2.0");
        assert_eq!(id.object_type, "obj_Activity");
        assert_eq!(id.qualified_type(), "resqml20.obj_Activity");
    }

    #[test]
    fn etp_uri_round_trips() {
        let uuid = "0e2ec4e1-1b0c-4b9f-8e9f-2a0d4c1f6a11";
        let uri = format!("eml:///dataspace('demo')/resqml22.TriangulatedSetRepresentation({uuid})");
        let (ds, id) = ObjectId::from_etp_uri(&uri).unwrap();
        assert_eq!(ds.as_deref(), Some("demo"));
        assert_eq!(id.object_type, "TriangulatedSetRepresentation");
        assert_eq!(id.etp_uri(Some("demo")), uri);
    }

    #[test]
    fn part_names_follow_the_type_verbatim() {
        let id = ObjectId::from_qualified_type("resqml20.obj_Activity", "u1").unwrap();
        assert_eq!(id.as_key().part_name(), "obj_Activity_u1.xml");
        assert_eq!(
            id.as_key().part_path(EpcLayout::PerPackage),
            "resqml20/obj_Activity_u1.xml"
        );
    }

    #[test]
    fn concepts_match_across_versions() {
        let old = ObjectId::from_qualified_type("resqml20.obj_Activity", "u").unwrap();
        let new = ObjectId::from_qualified_type("resqml22.Activity", "u").unwrap();
        assert!(old.as_key().same_concept(&new.as_key()));
        assert_eq!(old.type_key(), new.type_key());
    }

    /// The version rule, once: a named version is read, a short one is accepted
    /// when nothing contradicts it, and two named versions that differ are two
    /// objects (the project's version rule, ADR-0003 and ADR-0004).
    #[test]
    fn a_named_version_is_read_and_a_short_one_is_accepted_when_nothing_contradicts_it() {
        use VersionFit::*;
        assert_eq!(version_fit(Some("1"), Some("1")), Exact);
        assert_eq!(version_fit(Some("1"), Some("2")), Contradicts);
        assert_eq!(version_fit(Some("1"), None), Compatible);
        assert_eq!(version_fit(None, Some("2")), Compatible);
        assert_eq!(version_fit(None, None), Exact);
        assert_eq!(
            version_fit(Some(" 1 "), Some("1")),
            Exact,
            "spaces are not part of a version"
        );
        assert_eq!(
            version_fit(Some("1"), Some("  ")),
            Compatible,
            "a blank version says nothing"
        );
        assert!(versions_agree(None, Some("2")) && !versions_agree(Some("1"), Some("2")));
    }

    #[test]
    fn uuid_case_does_not_split_identities() {
        let a = ObjectId::from_qualified_type("resqml22.Activity", "ABCD").unwrap();
        let b = ObjectId::from_qualified_type("resqml22.Activity", "abcd").unwrap();
        assert_eq!(a, b);
    }

    const UUID: &str = "0e2ec4e1-1b0c-4b9f-8e9f-2a0d4c1f6a11";

    /// Decision D1: `from_etp_uri` builds its identity from what
    /// `EtpUri::parse` returns. The object version, the dataspace and the
    /// lowercase uuid are the door's.
    #[test]
    fn from_etp_uri_is_built_from_the_door_s_answer() {
        use crate::identifiers::EtpUri;
        let text = format!(
            "eml:///dataspace('demo')/resqml20.obj_Activity(uuid={},version='O''Neil')",
            UUID.to_ascii_uppercase()
        );
        let door = EtpUri::parse(&text).unwrap();
        let (dataspace, id) = ObjectId::from_etp_uri(&text).unwrap();
        assert_eq!(dataspace, door.dataspace);
        assert_eq!(
            Some(id.uuid.as_str()),
            door.uuid.as_deref(),
            "the door lowercases it"
        );
        assert_eq!(id.object_version, door.object_version);
        assert_eq!(id.object_version.as_deref(), Some("O'Neil"));
        assert_eq!(id.version, "2.0");
        assert_eq!(id.object_type, "obj_Activity");
        // Break it: read the uri again by hand in `from_etp_uri`, and keep the uuid as written.
    }

    /// The inputs the old hand-written splitter took and the door refuses are
    /// refused by name - a uuid that is not a uuid, a type with a character a
    /// type cannot carry - and so are the two shapes the door reads and an
    /// identity cannot hold.
    #[test]
    fn from_etp_uri_refuses_what_the_door_refuses_and_what_is_no_object() {
        for refused in [
            "eml:///resqml22.Activity(not-a-uuid)".to_string(),
            format!("eml:///resqml22.Foo-Bar({UUID})"),
            format!("eml:///resqml22.Activity({UUID}"),
            "eml:///dataspace('demo')".to_string(),
            format!("eml:///resqml22.Activity({UUID})/resqml22.Other?query"),
            format!("resqml22.Activity({UUID})"),
        ] {
            assert!(ObjectId::from_etp_uri(&refused).is_err(), "{refused}");
        }
        // The `uuid=` form, which the door reads, is read here too.
        let (_, id) = ObjectId::from_etp_uri(&format!("eml:///resqml22.Activity(uuid={UUID})")).unwrap();
        assert_eq!(id.uuid, UUID);
        assert_eq!(id.object_version, None);
        // Break it: let `from_etp_uri` take the uuid text without the door.
    }

    /// One grammar for a qualified type and for a content type: the owned and
    /// the borrowed readers accept and refuse the same strings.
    #[test]
    fn the_owned_and_the_borrowed_parsers_agree() {
        use crate::identifiers::{ContentType, QualifiedType};
        for qualified in [
            "resqml22.Activity",
            "resqml20.obj_Activity",
            " eml23.Foo ",
            "resqml22.Foo-Bar",
            "resqml22.Foo.Bar",
            "re_sqml22.Foo",
            "resqml.Foo",
            "resqml22.",
            "22.Foo",
        ] {
            assert_eq!(
                QualifiedType::parse(qualified).is_ok(),
                parse_qualified_type(qualified).is_ok(),
                "{qualified}"
            );
        }
        assert!(parse_qualified_type("resqml22.Foo-Bar").is_err());
        assert!(parse_qualified_type("resqml22.Foo.Bar").is_err());
        for content in [
            "application/x-resqml+xml;version=2.0;type=obj_Activity",
            "application/x-resqml+xml; version=2.0 ; type=obj_Activity",
            "application/x-resqml+xml;type=obj_Activity;version=2.0",
            "application/x-resqml+xml;version=2.0",
            "application/x-hdf5;version=1.0",
            "application/x-resqml+xml;version=2.0;type=obj Activity",
            "application/x-resqml+xml;version=;type=obj_Activity",
            "text/plain",
        ] {
            let owned = ContentType::parse(content);
            let borrowed = parse_content_type(content);
            // The borrowed reader wants an object type and a domain besides.
            assert_eq!(
                borrowed.is_ok(),
                owned
                    .as_ref()
                    .is_ok_and(|c| c.object_type.is_some() && c.raw_mime.is_none()),
                "{content}"
            );
            if let (Ok(owned), Ok(borrowed)) = (owned, borrowed) {
                assert_eq!(owned.domain, borrowed.domain);
                assert_eq!(owned.version, borrowed.version);
            }
        }
        // Break it: let `parse_qualified_type` accept a `.` inside the type again.
    }

    /// Two addresses that name no object are not the same object: `None ==
    /// None` is no identity (a dataspace uri carries no uuid).
    #[test]
    fn two_addresses_that_name_no_object_are_not_the_same_object() {
        assert!(!same_address("eml:///dataspace('a')", "eml:///dataspace('b')"));
        assert!(!same_address("eml:///dataspace('a')", "eml:///dataspace('a')"));
        assert!(!same_address("", ""));
        let one = format!("eml:///resqml22.Activity({UUID})");
        let upper = format!(
            "eml:///dataspace('x')/resqml22.Activity({})",
            UUID.to_ascii_uppercase()
        );
        let versioned = format!("eml:///resqml22.Activity(uuid={UUID},version='2')");
        let other_version = format!("eml:///resqml22.Activity(uuid={UUID},version='3')");
        assert!(
            same_address(&one, &upper),
            "the case of a uuid and the dataspace do not split it"
        );
        assert!(same_address(&one, &versioned), "a short uri matches any version");
        assert!(!same_address(&versioned, &other_version));
        assert!(same_address(UUID, &one), "a bare uuid is read as the uuid it is");
        // Break it: drop the `names_an_object` test in `same_address`.
    }
}
