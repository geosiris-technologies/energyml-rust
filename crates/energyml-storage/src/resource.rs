//! What a store knows about an object before it reads it.
//!
//! Three levels of materialisation, and a listing needs all three (see
//! ADR-0002):
//!
//! 1. [`ObjectSummary`] - identity only, from the package index. No object XML
//!    is opened at all.
//! 2. [`ObjectHeader`] - plus the citation. Read by stopping the XML parser as
//!    soon as the citation has gone by; the rest of the part is never parsed.
//! 3. The object itself, through the registry.
//!
//! An interface listing a two-gigabyte EPC lives at level 2. Getting there
//! cheaply is the whole point.

/// One data store: an EPC file, or a remote dataspace.
///
/// An EPC **is** a dataspace, holding exactly one. That is not a convenience:
/// it means code walking a remote dataspace walks a file without knowing it.
///
/// `#[non_exhaustive]`, like every record of this module: built with
/// [`Dataspace::new`] and its fields set afterwards, so that a field ETP needs
/// tomorrow is not a breaking change for every caller.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Dataspace {
    /// URI of the dataspace: `eml:///dataspace('name')`, or `eml:///` for the
    /// default one.
    pub uri: String,
    /// Human-readable name. For an EPC, the file name.
    pub name: String,
    /// When it was created, in its lexical form, when the store knows.
    pub created: Option<String>,
    /// When it last changed, in its lexical form, when the store knows.
    pub last_changed: Option<String>,
    /// ETP `Dataspace.storeCreated`: when **this store** created the dataspace,
    /// as the ETP timestamp (microseconds since the Unix epoch, UTC). `None` when
    /// the store does not keep it - an EPC does not, and says so rather than
    /// inventing a date.
    pub store_created: Option<i64>,
    /// ETP `Dataspace.storeLastWrite`: when this store last wrote into it, same
    /// unit. `None` when the store does not keep it.
    pub store_last_write: Option<i64>,
    /// ETP `Dataspace.customData`: what a server attaches to a dataspace beyond
    /// ETP's own fields - OSDU's access lists and legal tags (`viewers`,
    /// `owners`, `legaltags`) travel here. Empty when there is none.
    pub custom_data: std::collections::BTreeMap<String, CustomValue>,
}

/// One value of ETP's `customData` (its `DataValue`), the forms a store has
/// been seen to attach to a dataspace. A floating-point value is not among them
/// on purpose: it would make every record that carries custom data lose `Eq`,
/// and none is known to be needed; a store meeting one refuses it by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomValue {
    /// A text.
    Text(String),
    /// A list of texts (an access list, a list of legal tags).
    Texts(Vec<String>),
    /// A boolean.
    Bool(bool),
    /// An integer.
    Integer(i64),
}

impl Dataspace {
    /// A dataspace by its URI and name; everything the store may not know is
    /// left unknown.
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            created: None,
            last_changed: None,
            store_created: None,
            store_last_write: None,
            custom_data: std::collections::BTreeMap::new(),
        }
    }
}

/// ETP `ActiveStatusKind`: whether an object is still growing.
///
/// Only a growing object (a WITSML log, a trajectory being drilled) is ever
/// active; ETP 1.2 carries the status on every resource. `TODO(verify)`: that
/// every object which cannot grow is `Inactive` (ETP 1.2 §3.12 and the growing
/// object protocol); until then a store that does not know leaves it unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActiveStatus {
    /// Still being written.
    Active,
    /// Not, or no longer, being written.
    Inactive,
}

/// Level 1: an object's identity, without opening it.
///
/// Built with [`ObjectSummary::new`]; `#[non_exhaustive]` (see [`Dataspace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ObjectSummary {
    /// ETP URI, the way every layer above addresses this object.
    pub uri: String,
    /// UUID, as written.
    pub uuid: String,
    /// `resqml20.obj_Activity` - the identifier used across versions.
    pub qualified_type: String,
    /// The full content type, as the package declares it.
    pub content_type: String,
    /// Where the object lives inside the store. For an EPC, the part name.
    ///
    /// Deliberately not a path a caller should build on: two EPC files may
    /// disagree on layout, and the specification allows both.
    pub locator: String,
    /// ETP `Resource.alternateUris`: other URIs the store answers to for this
    /// object. Empty when it names none.
    pub alternate_uris: Vec<String>,
    /// ETP `Resource.storeCreated`: when this store first held the object, as
    /// the ETP timestamp (microseconds since the Unix epoch, UTC). `None` when
    /// the store does not keep it.
    pub store_created: Option<i64>,
    /// ETP `Resource.storeLastWrite`: when this store last wrote it, same unit.
    /// `None` when the store does not keep it.
    pub store_last_write: Option<i64>,
    /// ETP `Resource.activeStatus`. `None` when the store does not say.
    pub active_status: Option<ActiveStatus>,
    /// ETP `Resource.sourceCount`: how many objects point at this one, counted
    /// over the relationships the store states ([`crate::EnergymlStorage::edges`]).
    /// `None` unless a context asked for counts (`Context::count_objects`).
    pub source_count: Option<u32>,
    /// ETP `Resource.targetCount`: how many objects this one points at, same
    /// counting. `None` unless asked.
    pub target_count: Option<u32>,
}

impl ObjectSummary {
    /// An object's identity; the content type and the locator are empty until
    /// set, and everything the store may not know is left unknown.
    pub fn new(uri: impl Into<String>, uuid: impl Into<String>, qualified_type: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            uuid: uuid.into(),
            qualified_type: qualified_type.into(),
            content_type: String::new(),
            locator: String::new(),
            alternate_uris: Vec::new(),
            store_created: None,
            store_last_write: None,
            active_status: None,
            source_count: None,
            target_count: None,
        }
    }

    /// With the content type the package declares.
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = content_type.into();
        self
    }

    /// With where the object lives inside the store.
    pub fn with_locator(mut self, locator: impl Into<String>) -> Self {
        self.locator = locator.into();
        self
    }
}

/// Level 2: the identity, plus what a citation carries.
///
/// Everything here comes from the first few hundred bytes of a part. Nothing
/// below the citation is parsed.
///
/// Built with [`ObjectHeader::new`]; `#[non_exhaustive]` (see [`Dataspace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ObjectHeader {
    /// Level 1.
    pub summary: ObjectSummary,
    /// `Citation.Title` - the name a user recognises.
    pub title: Option<String>,
    /// Who or what produced the object.
    pub originator: Option<String>,
    /// Creation timestamp, in its lexical form.
    pub creation: Option<String>,
    /// Last update timestamp, in its lexical form.
    pub last_update: Option<String>,
    /// The producing software, as it named itself.
    pub format: Option<String>,
    /// `objectVersion`, when the object carries one.
    pub object_version: Option<String>,
    /// Schema version as written in the document, e.g. `2.0`.
    pub schema_version: Option<String>,
}

impl ObjectHeader {
    /// The header of an object whose citation is not known yet: every field of
    /// it unset.
    pub fn new(summary: ObjectSummary) -> Self {
        Self {
            summary,
            title: None,
            originator: None,
            creation: None,
            last_update: None,
            format: None,
            object_version: None,
            schema_version: None,
        }
    }

    /// With a title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Identity of the object.
    pub fn uri(&self) -> &str {
        &self.summary.uri
    }

    /// Title, or the uuid when the citation has none - a UI needs *something*
    /// to show, and an empty row is worse than a technical one.
    pub fn display_name(&self) -> &str {
        match self.title.as_deref() {
            Some(title) if !title.trim().is_empty() => title,
            _ => &self.summary.uuid,
        }
    }

    /// The title (the uuid when there is none) followed by ` (version N)` when
    /// the object declares a non-blank `objectVersion` - what a column that
    /// already has its own type column shows. The same rule as [`Self::label`].
    pub fn titled(&self) -> String {
        match self
            .object_version
            .as_deref()
            .map(str::trim)
            .filter(|version| !version.is_empty())
        {
            Some(version) => format!("{} (version {version})", self.display_name()),
            None => self.display_name().to_string(),
        }
    }

    /// What a view shows of the object: `type - title (version N)`, the
    /// version only when the object declares a non-blank one -
    /// [`energyml_core::identity::ObjectId::label`], the one spelling every
    /// face uses. The qualified type alone when it cannot be parsed.
    pub fn label(&self) -> String {
        match energyml_core::identity::ObjectId::from_qualified_type(
            &self.summary.qualified_type,
            &self.summary.uuid,
        ) {
            Ok(mut id) => {
                id.object_version = self.object_version.clone();
                id.label(self.title.as_deref())
            }
            Err(_) => format!("{} - {}", self.summary.qualified_type, self.display_name()),
        }
    }
}

/// One relationship between two parts, as the package records it.
///
/// Kept as read: the relationship system is an index, and an index can be
/// wrong. Compare it with what the objects say (`identity::collect_refs`)
/// rather than trusting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Relationship id, unique within its `.rels` part.
    pub id: String,
    /// Full type URI, e.g.
    /// `http://schemas.energistics.org/package/2012/relationships/destinationObject`.
    pub kind: String,
    /// What it points at: a part name, or a file name when external.
    pub target: String,
    /// Whether the target lies outside the package.
    pub external: bool,
}

impl Relationship {
    /// The Energistics relationship types, as short names.
    ///
    /// The full form is `http://schemas.energistics.org/package/2012/relationships/<name>`
    /// (EPC specification §3.4.2).
    pub fn short_kind(&self) -> &str {
        self.kind.rsplit('/').next().unwrap_or(&self.kind)
    }

    /// Whether this relationship points at a file outside the package -
    /// an HDF5 file, typically.
    pub fn is_external_resource(&self) -> bool {
        self.external || self.short_kind() == "externalResource"
    }
}

/// Whether a qualified type contains this filter, ignoring case.
///
/// **One spelling, because four had grown**: `list`, `show` and `geometry` in
/// the command line each lowercased their own way, and the Python bridge was
/// about to make a fourth. The filter is a substring and not a type name on
/// purpose - `Representation` is the useful question, and nobody wants to type
/// `resqml22.TriangulatedSetRepresentation` to ask it.
pub fn type_matches(qualified_type: &str, filter: &str) -> bool {
    qualified_type.to_lowercase().contains(&filter.to_lowercase())
}

/// Whether a citation title contains this filter, ignoring case.
///
/// Beside [`type_matches`] and for the same reason. A title is not in an
/// [`ObjectSummary`] - reading one costs a header pass - so this takes the
/// title the caller already has.
pub fn title_matches(title: &str, filter: &str) -> bool {
    title.to_lowercase().contains(&filter.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(title: Option<&str>, version: Option<&str>) -> ObjectHeader {
        let mut header = ObjectHeader::new(ObjectSummary::new(
            "eml:///resqml22.BoundaryFeature(7f000000-0000-4000-8000-000000000001)",
            "7f000000-0000-4000-8000-000000000001",
            "resqml22.BoundaryFeature",
        ));
        header.title = title.map(str::to_string);
        header.object_version = version.map(str::to_string);
        header
    }

    #[test]
    fn a_view_shows_the_version_when_the_object_declares_one() {
        let versioned = header(Some("Top Brent"), Some("2"));
        assert_eq!(
            versioned.label(),
            "resqml22.BoundaryFeature - Top Brent (version 2)"
        );
        assert_eq!(versioned.titled(), "Top Brent (version 2)");
    }

    #[test]
    fn no_version_a_blank_one_or_no_title_changes_nothing_but_what_is_missing() {
        for none in [None, Some(""), Some("   ")] {
            let plain = header(Some("Top Brent"), none);
            assert_eq!(plain.label(), "resqml22.BoundaryFeature - Top Brent", "{none:?}");
            assert_eq!(plain.titled(), "Top Brent", "{none:?}");
        }
        // No title: the uuid stands in, and the version still shows.
        let untitled = header(None, Some("1"));
        assert_eq!(
            untitled.titled(),
            "7f000000-0000-4000-8000-000000000001 (version 1)"
        );
    }
}
