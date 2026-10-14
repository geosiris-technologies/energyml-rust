//! Checking that the references inside a package point somewhere.
//!
//! A `DataObjectReference` carries three things: a uuid, a declared type, and a
//! title. Nothing keeps them in agreement with the object they name - the uuid
//! can point at a part that was never packaged, the declared type can be the
//! one the writer *thought* it was, and both mistakes survive every XSD
//! validator ever written, because each reference is well-formed on its own.
//!
//! These are the checks a `.rels` file cannot make either: the relationship
//! system is an index built by the same tool that got the reference wrong.
//! What an object actually references comes from its DORs, which is why this
//! walks reflection rather than reading `.rels` (see
//! ADR-0002: the `.rels` speed things up, the DORs are authoritative).
//!
//! Free functions over objects **already in memory**, deliberately: no store,
//! no I/O, no async colour. The caller reads, this decides.

use std::fmt;

use energyml_core::identifiers;
use energyml_core::identity;
use energyml_core::naming;
use energyml_core::value::Reflect;

use crate::resource::{ObjectSummary, Relationship};

/// Something wrong with one reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefIssue {
    /// URI of the object holding the reference.
    pub source: String,
    /// Where inside it: `Grid2dPatch.SupportingRepresentation`.
    pub path: String,
    /// The uuid the reference names.
    pub target_uuid: String,
    /// The type the reference claims its target has, when it declares one.
    pub declared_type: Option<String>,
    /// What is wrong.
    pub kind: RefIssueKind,
}

/// The four ways a reference is worth reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefIssueKind {
    /// No object with that uuid in the package.
    ///
    /// Not always an error: an EPC may legitimately reference an object held
    /// elsewhere. It is always worth knowing, and it is what breaks a viewer
    /// that assumes a package is self-contained.
    Dangling,
    /// The target is absent, and absence is not evidence of anything: the
    /// concept it names is defined in a catalogue Energistics publishes, not
    /// in a dataspace.
    ///
    /// `PropertyKind` is the case, and it is not marginal. The dictionary
    /// shipped with the standard defines several hundred of them and nothing
    /// asks a package to embed the ones it uses - a package of 120 wellbores
    /// produced 1 086 of these and not one genuine dangling reference, which
    /// is precisely how a real one gets buried.
    ///
    /// Whether the uuid *is* in the dictionary is a different question, and it
    /// belongs to the layer that owns the dictionary
    /// (`energyml_domain::property_kind::Resolver`, which `energyml check`
    /// already runs). This variant only says: not being here proves nothing.
    ExternallyDefined,
    /// The target exists but is a different *concept* - the reference says
    /// `WellboreInterpretation`, the part is a `HorizonInterpretation`.
    ///
    /// Always a real error: whatever reads this will follow the reference and
    /// find something it cannot use.
    ConceptMismatch {
        /// Qualified type of the nearest object found.
        found: String,
        /// The **full uri of every object that carries the uuid** - none of them
        /// is what the reference names. The message built from them reads
        /// 'uuid found but type or version should be one of the following: '
        /// followed by the uris of the objects that carry the right uuid but
        /// not the right type or version number. Sorted.
        candidates: Vec<String>,
    },
    /// Same concept, different version - `resqml20.obj_Activity` naming a
    /// `resqml22.Activity`.
    ///
    /// A warning, not an error. A package may mix versions (EPC §3.2), and a
    /// reader that matches on uuid finds the right object; but a reader that
    /// trusts the declared type to pick a parser does not.
    VersionMismatch {
        /// Qualified type of the nearest object found.
        found: String,
        /// The full uri of every object that carries the uuid, sorted.
        candidates: Vec<String>,
    },
    /// The right object, at another revision: the reference's
    /// `ObjectVersion` and the object's `objectVersion` disagree.
    ///
    /// An **error** when the reference names a revision - one the object is
    /// not at, or one where the object carries none: whatever follows it is
    /// promised a revision the package does not hold. A **warning** when the
    /// reference names none and the object does: the Identifier
    /// Specification has a reader assume *the latest*, which is what the
    /// package holds, so nothing breaks - but a package that stamps every
    /// object with a version and writes its references without one says two
    /// things, and the Webstudio flags it. It was exactly
    /// what our own translation produced, and this checker said nothing.
    ObjectVersionMismatch {
        /// What the reference says, when it says anything.
        referenced: Option<String>,
        /// What the object carries, when it carries anything. `None` also when
        /// several entries of the uuid and type are held and none is *the*
        /// object - see `candidates`.
        found: Option<String>,
        /// The full uri of every object that carries the uuid, sorted. A
        /// reference resolves **exactly** - by uuid, type and version - and when
        /// nothing answers, this is the list a person picks from.
        candidates: Vec<String>,
    },
    /// The right object, under another name: the reference's cached `Title`
    /// and the object's citation title disagree.
    ///
    /// A **warning**, never an error: a reader resolves by uuid and finds the
    /// right object, so nothing breaks. What it does break is every list built
    /// from references rather than from the objects - a graph, a tree, a
    /// picker - which then shows a name the package no longer uses, or no name
    /// at all. The Webstudio reports it, and this checker did not (seen on a
    /// package our own translation wrote: the two references of a
    /// `LocalEngineeringCompoundCrs` carried an empty title).
    TitleMismatch {
        /// What the reference calls it, when it calls it anything.
        referenced: Option<String>,
        /// What the object calls itself.
        found: String,
    },
    /// The reference names a type **no schema this build carries defines** -
    /// well formed (`resqml22.NoSuchThing` matches the `QualifiedType`
    /// pattern) and pointing at nothing a reader could ever open.
    ///
    /// A **warning**: the pattern accepts any two-digit version, so a package
    /// written against a release this build does not carry says the same thing.
    /// It is the one reference finding no store can make - it needs the
    /// registry of types, which is why it is [`check_declared_types`] and not
    /// part of [`check_references`].
    UnknownType,
}

impl RefIssue {
    /// Whether this breaks the package, as opposed to merely being odd.
    pub fn is_error(&self) -> bool {
        matches!(
            self.kind,
            RefIssueKind::Dangling
                | RefIssueKind::ConceptMismatch { .. }
                | RefIssueKind::ObjectVersionMismatch {
                    referenced: Some(_),
                    ..
                }
        )
    }

    /// Whether this is worth printing but says nothing is wrong.
    ///
    /// Separate from `!is_error()`: a `VersionMismatch` is a warning about the
    /// *package*, an `ExternallyDefined` is a remark about how the standard is
    /// published. Counting them together would put a thousand remarks in the
    /// warning column and hide the handful that mean something.
    pub fn is_info(&self) -> bool {
        matches!(self.kind, RefIssueKind::ExternallyDefined)
    }
}

impl fmt::Display for RefIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.path)?;
        match &self.kind {
            RefIssueKind::Dangling => match &self.declared_type {
                Some(declared) => {
                    write!(
                        f,
                        "references {} ({}), which is not in this package",
                        self.target_uuid, declared
                    )
                }
                None => write!(f, "references {}, which is not in this package", self.target_uuid),
            },
            RefIssueKind::ExternallyDefined => write!(
                f,
                "references {} ({}), which the Energistics catalogue defines - a package need not carry it",
                self.target_uuid,
                self.declared_type.as_deref().unwrap_or("?")
            ),
            RefIssueKind::ConceptMismatch { found, candidates } => write!(
                f,
                "references {} as `{}`, but that object is a `{found}`{}",
                self.target_uuid,
                self.declared_type.as_deref().unwrap_or("?"),
                should_be_one_of(candidates)
            ),
            RefIssueKind::VersionMismatch { found, candidates } => write!(
                f,
                "references {} as `{}`, but that object is a `{found}` - same concept, other version{}",
                self.target_uuid,
                self.declared_type.as_deref().unwrap_or("?"),
                should_be_one_of(candidates)
            ),
            RefIssueKind::ObjectVersionMismatch {
                referenced: Some(referenced),
                found: Some(found),
                candidates,
            } => write!(
                f,
                "references {} at version `{referenced}`, but that object is at version `{found}`{}",
                self.target_uuid,
                should_be_one_of(candidates)
            ),
            RefIssueKind::ObjectVersionMismatch {
                referenced: Some(referenced),
                found: None,
                candidates,
            } => write!(
                f,
                "references {} at version `{referenced}`, but {}{}",
                self.target_uuid,
                if candidates.len() > 1 {
                    "no object of that uuid and type carries it"
                } else {
                    "that object carries no objectVersion"
                },
                should_be_one_of(candidates)
            ),
            RefIssueKind::ObjectVersionMismatch {
                referenced: None,
                found,
                candidates,
            } if candidates.len() > 1 => write!(
                f,
                "references {} without an ObjectVersion, and {} objects of that uuid and type are held{}{}",
                self.target_uuid,
                candidates.len(),
                found
                    .as_deref()
                    .map(|version| format!(" (the nearest at version `{version}`)"))
                    .unwrap_or_default(),
                should_be_one_of(candidates)
            ),
            RefIssueKind::ObjectVersionMismatch {
                referenced: None,
                found,
                ..
            } => write!(
                f,
                "references {} without an ObjectVersion, and that object is at version `{}` - a reader assumes the latest, which is this one; say it",
                self.target_uuid,
                found.as_deref().unwrap_or("?")
            ),
            RefIssueKind::TitleMismatch {
                referenced: Some(referenced),
                found,
            } => write!(
                f,
                "calls {} `{referenced}`, and that object calls itself `{found}` - the title in a reference is a copy, and this one is stale",
                self.target_uuid
            ),
            RefIssueKind::TitleMismatch {
                referenced: None,
                found,
            } => write!(
                f,
                "references {} with no title, and that object is called `{found}` - the title in a reference is a copy, and this one is empty",
                self.target_uuid
            ),
            RefIssueKind::UnknownType => write!(
                f,
                "references {} as `{}`, a type no schema known to this build defines",
                self.target_uuid,
                self.declared_type.as_deref().unwrap_or("?")
            ),
        }
    }
}

/// The sentence that names the candidates,
/// or nothing when there are none to name.
fn should_be_one_of(candidates: &[String]) -> String {
    if candidates.is_empty() {
        String::new()
    } else {
        format!(
            ": uuid found but type or version should be one of the following: {}",
            candidates.join(", ")
        )
    }
}

/// What a reference caches about its target, as the **target** states it.
///
/// Two answers and not an `Option`, because *not read* and *states none* are
/// different news: the first says nothing, the second is compared. A level-1
/// listing reads no part, so a caller with only summaries answers
/// [`Cached::Unread`] and gets no finding about either field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    /// The object was not read: nothing to compare, nothing said.
    Unread,
    /// Read, and this is what it says of itself.
    Read {
        /// Its `objectVersion`, `None` when it carries none.
        object_version: Option<String>,
        /// Its citation title, `None` when it has none.
        title: Option<String>,
    },
}

/// The reference's `ObjectVersion` against the object's - see
/// [`RefIssueKind::ObjectVersionMismatch`] for which way each case goes.
fn revision_check(referenced: Option<&str>, found: &Cached, candidates: &[String]) -> Option<RefIssueKind> {
    let Cached::Read {
        object_version: found,
        ..
    } = found
    else {
        return None;
    };
    let referenced = referenced.map(str::trim).filter(|v| !v.is_empty());
    let found = found.as_deref().map(str::trim).filter(|v| !v.is_empty());
    match (referenced, found) {
        (None, None) => None,
        (Some(r), Some(f)) if r == f => None,
        (r, f) => Some(RefIssueKind::ObjectVersionMismatch {
            referenced: r.map(str::to_string),
            found: f.map(str::to_string),
            candidates: candidates.to_vec(),
        }),
    }
}

/// The `objectVersion` an entry states, as the target says of itself: the
/// outer `None` is *not read* (nothing can be compared), the inner one *carries
/// none*.
fn stated_version(
    target: &ObjectSummary,
    cached_of: &dyn Fn(&ObjectSummary) -> Cached,
) -> Option<Option<String>> {
    match cached_of(target) {
        // Not read - but the store spells **the version into the uri** of an entry
        // that declares one, so a uri that carries a version says it. One that
        // carries none says nothing: *not read* and *states none* differ.
        Cached::Unread => match identity::ObjectId::parse(&target.uri) {
            Ok((_, id))
                if id
                    .object_version
                    .as_deref()
                    .is_some_and(|version| !version.trim().is_empty()) =>
            {
                Some(id.object_version.map(|version| version.trim().to_string()))
            }
            _ => None,
        },
        Cached::Read { object_version, .. } => Some(
            object_version
                .map(|version| version.trim().to_string())
                .filter(|version| !version.is_empty()),
        ),
    }
}

/// The reference's `Title` against the object's citation title.
///
/// Nothing is said when the target states no title of its own: there is then
/// no original for the copy to disagree with, and a reference that names one
/// anyway is the only record of a name the package has otherwise lost.
fn title_check(referenced: Option<&str>, found: &Cached) -> Option<RefIssueKind> {
    let Cached::Read {
        title: Some(found), ..
    } = found
    else {
        return None;
    };
    let found = found.trim();
    if found.is_empty() {
        return None;
    }
    let referenced = referenced.map(str::trim).filter(|value| !value.is_empty());
    match referenced {
        Some(value) if value == found => None,
        other => Some(RefIssueKind::TitleMismatch {
            referenced: other.map(str::to_string),
            found: found.to_string(),
        }),
    }
}

/// Check every reference one object holds against what the package contains.
///
/// `known` answers "is there an object with this uuid, and what is it?" - a
/// closure rather than a store, so this function has no idea whether the
/// package is a file, a dataspace or a test fixture. Its lifetime is named
/// because elision would otherwise tie the answer to the uuid that was asked
/// about, which is not where the summary lives.
///
/// ```no_run
/// # use energyml_storage::{EpcStore, graph};
/// # fn demo(store: &EpcStore, summary: &energyml_storage::ObjectSummary) -> energyml_storage::Result<()> {
/// let object = store.get_object(&summary.uri)?;
/// let issues = graph::check_references(summary, object.as_reflect(), &|uuid| store.find_by_uuid(uuid), &|_| graph::Cached::Unread);
/// # Ok(())
/// # }
/// ```
///
/// The borrow is not an EPC assumption. A store that hands its inventory over
/// by value - as [`crate::storage::EnergymlStorage::resources`] does, and as an
/// ETP client must - indexes the `Vec` it owns and borrows from that:
///
/// ```no_run
/// # use energyml_storage::{ObjectSummary, graph};
/// # fn demo(resources: Vec<ObjectSummary>, summary: &ObjectSummary, object: &dyn energyml_core::value::Reflect) {
/// let by_uuid: std::collections::HashMap<String, &ObjectSummary> = resources
///     .iter()
///     .map(|s| (s.uuid.to_ascii_lowercase(), s))
///     .collect();
/// let issues = graph::check_references(
///     summary,
///     object,
///     &|uuid| by_uuid.get(&uuid.to_ascii_lowercase()).copied(),
///     &|_| graph::Cached::Unread,
/// );
/// # let _ = issues;
/// # }
/// ```
///
/// `cached_of` answers what the target says of itself - its `objectVersion`
/// and its title - when the caller has read the object; the summary carries
/// neither, since a level-1 listing reads no part. [`check_all_references`]
/// answers it from the objects it is given; a caller with none to hand answers
/// [`Cached::Unread`] and gets no finding about either.
pub fn check_references<'i>(
    source: &ObjectSummary,
    object: &dyn Reflect,
    known: &dyn Fn(&str) -> Option<&'i ObjectSummary>,
    cached_of: &dyn Fn(&str) -> Cached,
) -> Vec<RefIssue> {
    check_references_by(
        source,
        object,
        &|uuid| known(uuid).into_iter().collect(),
        &|target| cached_of(&target.uuid),
    )
}

/// [`check_references`] for a package that may hold **one uuid several times** -
/// under two `objectVersion`s, or two releases. `held` is asked for **every
/// entry that carries the uuid** the reference names; the check then resolves
/// the reference **exactly**, by the identity read from it - uuid, qualified
/// type, `objectVersion` when it carries one - and when nothing answers while
/// the uuid is held, the finding names every candidate by its full uri.
/// `cached_of` is asked about an entry and not about its
/// uuid, so the revision of the reference is compared with the revision of the
/// object it names rather than with a namesake's.
pub fn check_references_by<'i>(
    source: &ObjectSummary,
    object: &dyn Reflect,
    held: &dyn Fn(&str) -> Vec<&'i ObjectSummary>,
    cached_of: &dyn Fn(&ObjectSummary) -> Cached,
) -> Vec<RefIssue> {
    let mut out = Vec::new();
    for edge in identity::collect_refs(object) {
        let uuid = edge.target.uuid;
        if uuid.is_empty() {
            continue;
        }
        let declared = edge.target.qualified_type();
        let issue = |kind| RefIssue {
            source: source.uri.clone(),
            path: edge.path.clone(),
            target_uuid: uuid.to_string(),
            declared_type: Some(declared.clone()),
            kind,
        };

        // Dangling is the one case where the **uuid itself** is absent.
        let candidates = held(uuid);
        if candidates.is_empty() {
            out.push(issue(if is_externally_defined(&declared) {
                RefIssueKind::ExternallyDefined
            } else {
                RefIssueKind::Dangling
            }));
            continue;
        }
        // Every entry of the uuid, by the uri that names it alone.
        let mut uris: Vec<String> = candidates.iter().map(|one| one.uri.clone()).collect();
        uris.sort();
        uris.dedup();
        let wanted = edge
            .target
            .object_version
            .map(str::trim)
            .filter(|version| !version.is_empty());

        // **Exact resolution**: the entries of the right type - *a wrong version
        // is not a wrong type*, so a release that differs is not exact - and,
        // among them, the one at the version the reference names.
        let same_type: Vec<&ObjectSummary> = candidates
            .iter()
            .copied()
            .filter(|one| same_qualified_type(&declared, &one.qualified_type))
            .collect();
        match exact_among(&same_type, wanted, cached_of) {
            Some(target) => {
                // The revision and the title: the four fields of a reference are
                // four copies, and each one can be stale on its own.
                let cached = cached_of(target);
                if let Some(kind) = revision_check(wanted, &cached, &uris) {
                    out.push(issue(kind));
                }
                if let Some(kind) = title_check(edge.target.title, &cached) {
                    out.push(issue(kind));
                }
            }
            // The right type, and no entry at the version named - or several
            // and none that is *the* one a reference without a version means.
            None if !same_type.is_empty() => {
                let sole = if same_type.len() == 1 {
                    Some(same_type[0])
                } else {
                    None
                };
                let found = sole.and_then(|one| stated_version(one, cached_of)).flatten();
                out.push(issue(RefIssueKind::ObjectVersionMismatch {
                    referenced: wanted.map(str::to_string),
                    found,
                    candidates: uris.clone(),
                }));
                if let Some(one) = sole
                    && let Some(kind) = title_check(edge.target.title, &cached_of(one))
                {
                    out.push(issue(kind));
                }
            }
            // No entry of the uuid has the type. Compare the concept first, the
            // version second: a package is allowed to mix versions, so
            // "different version" and "different thing" must not come out as the
            // same complaint - the nearest candidate decides which this is.
            None => {
                let near = candidates
                    .iter()
                    .copied()
                    .find(|one| same_concept(&declared, &one.qualified_type));
                let kind = match near {
                    Some(one) => RefIssueKind::VersionMismatch {
                        found: one.qualified_type.clone(),
                        candidates: uris.clone(),
                    },
                    None => RefIssueKind::ConceptMismatch {
                        found: candidates[0].qualified_type.clone(),
                        candidates: uris.clone(),
                    },
                };
                out.push(issue(kind));
                // One candidate is *the* object the reference means to name: its
                // revision and title are still compared.
                if let [only] = candidates[..] {
                    let cached = cached_of(only);
                    if let Some(kind) = revision_check(wanted, &cached, &uris) {
                        out.push(issue(kind));
                    }
                    if let Some(kind) = title_check(edge.target.title, &cached) {
                        out.push(issue(kind));
                    }
                }
            }
        }
    }
    out
}

/// The entry a reference names **exactly** among the entries of its uuid and
/// type: the one at the version it states, or - when it states none - the only
/// one, else the only one that declares none. `None` when nothing answers, and
/// then nothing is guessed: the finding lists the candidates instead. An entry
/// whose version was not read is not disqualified (nothing says it differs).
fn exact_among<'a>(
    same_type: &[&'a ObjectSummary],
    wanted: Option<&str>,
    cached_of: &dyn Fn(&ObjectSummary) -> Cached,
) -> Option<&'a ObjectSummary> {
    match wanted {
        Some(wanted) => same_type
            .iter()
            .copied()
            .find(|one| match stated_version(one, cached_of) {
                None => true,
                Some(version) => version.as_deref() == Some(wanted),
            }),
        None => match same_type {
            [only] => Some(*only),
            many => {
                let bare: Vec<&'a ObjectSummary> = many
                    .iter()
                    .copied()
                    .filter(|one| matches!(stated_version(one, cached_of), Some(None)))
                    .collect();
                match bare[..] {
                    [only] => Some(only),
                    _ => None,
                }
            }
        },
    }
}

/// Every reference that names a type `knows` does not recognise.
///
/// `knows` answers "does any schema define this qualified type?" - a closure,
/// so that the storage layer keeps no registry of its own. References to a
/// concept the standard publishes in a catalogue (`PropertyKind` and the like)
/// are skipped: their types are known, and absence of the object is not news.
pub fn check_declared_types<'a>(
    objects: impl IntoIterator<Item = (&'a ObjectSummary, &'a dyn Reflect)>,
    knows: &dyn Fn(&str) -> bool,
) -> Vec<RefIssue> {
    let mut out = Vec::new();
    for (source, object) in objects {
        for edge in identity::collect_refs(object) {
            if edge.target.uuid.is_empty() {
                continue;
            }
            let declared = edge.target.qualified_type();
            if knows(&declared) {
                continue;
            }
            out.push(RefIssue {
                source: source.uri.clone(),
                path: edge.path.clone(),
                target_uuid: edge.target.uuid.to_string(),
                declared_type: Some(declared),
                kind: RefIssueKind::UnknownType,
            });
        }
    }
    out
}

/// The data objects nested inside the objects given, by uuid - each with a
/// summary of its own that names **the part holding it** as its locator, and
/// the uuid of that part's object.
///
/// A reference may name a nested object: a WITSML `Channel` inside its
/// `ChannelSet`, which PRODML flow-test data references by uuid (see
/// [`identity::nested_objects`]). Such a reference resolves to the part that
/// holds it. An object that is a part of its own is never shadowed by a nested
/// copy of the same uuid.
pub fn nested_index<'a>(
    objects: impl IntoIterator<Item = (&'a ObjectSummary, &'a dyn Reflect)>,
) -> std::collections::HashMap<String, (ObjectSummary, String, Cached)> {
    let objects: Vec<(&ObjectSummary, &dyn Reflect)> = objects.into_iter().collect();
    let parts: std::collections::HashSet<String> = objects
        .iter()
        .map(|(summary, _)| summary.uuid.to_ascii_lowercase())
        .collect();
    let mut out = std::collections::HashMap::new();
    for (container, object) in objects {
        for nested in identity::nested_objects(object) {
            let uuid = nested.key.uuid.to_ascii_lowercase();
            if parts.contains(&uuid) {
                continue;
            }
            let summary = ObjectSummary::new(
                nested.key.etp_uri(None),
                nested.key.uuid.to_string(),
                nested.key.qualified_type(),
            )
            .with_content_type(nested.key.content_type())
            .with_locator(container.locator.clone());
            let cached = Cached::Read {
                object_version: nested.key.object_version.map(str::to_string),
                title: nested.key.title.map(str::to_string),
            };
            out.entry(uuid)
                .or_insert((summary, container.uuid.to_ascii_lowercase(), cached));
        }
    }
    out
}

/// Every reference in the package, checked in one pass.
///
/// `objects` pairs each summary with the object already deserialized, because
/// deciding when to read is the caller's business, not this function's.
///
/// The objects given are also what answers a target's revision: every one of
/// them has been read, so its `objectVersion` is at hand, and a target outside
/// the batch is `Unread`.
pub fn check_all_references<'a, 'i>(
    objects: impl IntoIterator<Item = (&'a ObjectSummary, &'a dyn Reflect)>,
    known: &dyn Fn(&str) -> Option<&'i ObjectSummary>,
) -> Vec<RefIssue> {
    check_all_references_with(objects, &|uuid| known(uuid).into_iter().collect())
}

/// [`check_all_references`] for a caller that can name **every** entry of a
/// uuid beyond the batch - the unreadable part of a package, a store's whole
/// listing. `elsewhere` answers all of them, not the first: a reference that
/// names a version no readable object carries is still answered by the
/// entries of its uuid, each by its full uri, and an entry that cannot be read
/// is not a reason to report the uuid dangling or to call it by another
/// version's name. Entries it names that the batch holds too (same uri) count
/// once.
pub fn check_all_references_with<'a, 'i>(
    objects: impl IntoIterator<Item = (&'a ObjectSummary, &'a dyn Reflect)>,
    elsewhere: &dyn Fn(&str) -> Vec<&'i ObjectSummary>,
) -> Vec<RefIssue> {
    let objects: Vec<(&ObjectSummary, &dyn Reflect)> = objects.into_iter().collect();
    // What each object says of itself, by **summary**, not by uuid: one uuid may
    // be held several times (two `objectVersion`s, two releases), and a
    // reference's revision has to be compared with the revision of the entry it
    // names.
    let stated: std::collections::HashMap<&str, Cached> = objects
        .iter()
        .map(|(summary, object)| {
            (
                summary.uri.as_str(),
                Cached::Read {
                    object_version: identity::object_version_of_reflect(*object).map(str::to_string),
                    title: identity::title_of_reflect(*object).map(str::to_string),
                },
            )
        })
        .collect();
    // The batch's own entries, by uuid: what a reference naming that uuid has to
    // choose among.
    let mut in_batch: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (at, (summary, _)) in objects.iter().enumerate() {
        in_batch
            .entry(summary.uuid.to_ascii_lowercase())
            .or_default()
            .push(at);
    }
    // A reference naming an object nested in one of these resolves to it
    // (see [`nested_index`]).
    let nested = nested_index(objects.iter().copied());
    let cached_of = |target: &ObjectSummary| {
        stated
            .get(target.uri.as_str())
            .cloned()
            .or_else(|| {
                nested
                    .get(&target.uuid.to_ascii_lowercase())
                    .map(|(_, _, cached)| cached.clone())
            })
            .unwrap_or(Cached::Unread)
    };
    // Every entry that carries the uuid: the batch's, then what the caller knows
    // beyond it (entries it could not read, another store's listing) - one per
    // uri - and, when nothing holds the uuid, the object nested in a part.
    let held = |uuid: &str| -> Vec<&ObjectSummary> {
        let key = uuid.to_ascii_lowercase();
        let mut out: Vec<&ObjectSummary> = in_batch
            .get(&key)
            .map(|ats| ats.iter().map(|at| objects[*at].0).collect())
            .unwrap_or_default();
        for other in elsewhere(uuid) {
            if !out.iter().any(|one| one.uri == other.uri) {
                out.push(other);
            }
        }
        if out.is_empty()
            && let Some((summary, _, _)) = nested.get(&key)
        {
            out.push(summary);
        }
        out
    };
    objects
        .iter()
        .flat_map(|(summary, object)| check_references_by(summary, *object, &held, &cached_of))
        .collect()
}

// ---------------------------------------------------------------------------
// Repair
// ---------------------------------------------------------------------------

/// What a repair pass did - or refused to do - to one reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefRepair {
    /// URI of the object holding the reference.
    pub source: String,
    /// Where inside it.
    pub path: String,
    /// The uuid the reference names.
    pub target_uuid: String,
    /// What happened.
    pub outcome: RepairOutcome,
}

/// The outcome of trying to bring one reference back in line with its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairOutcome {
    /// Rewritten from the target, which is the authority for its own type,
    /// title and revision.
    Done {
        /// What the declared type said before, when it said anything.
        was_type: Option<String>,
        /// What it says now.
        now_type: String,
        /// The title written, when the title was the thing that drifted.
        now_title: Option<String>,
        /// The `ObjectVersion` now written, when the target carries one.
        now_version: Option<String>,
    },
    /// Left alone, and why.
    Refused(Refusal),
}

/// Why a reference was not repaired.
///
/// Every variant is a case where **the package does not say which half is
/// wrong**. Rewriting one of them would not fix anything; it would make a
/// broken link look sound, which is worse than leaving it visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The uuid names nothing in the package. There is no target to copy from,
    /// and inventing one is not a repair.
    Dangling,
    /// The target exists but is another concept entirely. Either the uuid is a
    /// typo and the type is right, or the reverse - and nothing here can tell.
    /// Rewriting the type would point the reference confidently at the wrong
    /// object.
    ConceptMismatch {
        /// What the reference claims.
        declared: String,
        /// What the object with that uuid actually is.
        found: String,
    },
    /// The reference lives in a subtree the model could not type, kept as
    /// source text. It is *found* - the graph draws it - but reflection cannot
    /// walk down to it, so nothing can write to it either.
    /// `GraphicalInformationSet` is the case that exists in the corpus.
    Unreachable {
        /// What the walk said.
        reason: String,
    },
}

impl RefRepair {
    /// Whether anything was written.
    pub fn changed(&self) -> bool {
        matches!(self.outcome, RepairOutcome::Done { .. })
    }
}

impl fmt::Display for RefRepair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.path)?;
        match &self.outcome {
            RepairOutcome::Done {
                was_type,
                now_type,
                now_title,
                now_version,
            } => {
                match was_type {
                    Some(was) if was != now_type => write!(f, "`{was}` → `{now_type}`")?,
                    _ => write!(f, "`{now_type}`")?,
                }
                if let Some(title) = now_title {
                    write!(f, ", title → `{title}`")?;
                }
                match now_version {
                    Some(version) => write!(f, ", version → `{version}`"),
                    None => Ok(()),
                }
            }
            RepairOutcome::Refused(Refusal::Dangling) => {
                write!(f, "left alone - {} is not in this package", self.target_uuid)
            }
            RepairOutcome::Refused(Refusal::ConceptMismatch { declared, found }) => write!(
                f,
                "left alone - says `{declared}`, but {} is a `{found}`; which half is wrong is not \
                 something this package says",
                self.target_uuid
            ),
            RepairOutcome::Refused(Refusal::Unreachable { reason }) => {
                write!(f, "left alone - {reason}")
            }
        }
    }
}

/// What a repair needs to know about a reference's target.
///
/// A closure rather than a store, for the same reason [`check_references`] takes
/// one: this decides, the caller reads.
pub struct Target {
    /// The target's real qualified type.
    pub qualified_type: String,
    /// Its citation title, when it has one.
    pub title: Option<String>,
    /// Its `objectVersion`, when it carries one.
    ///
    /// `None` means *the object states no revision*, and a reference to it
    /// says none either - which is what the Identifier Specification reads as
    /// *the only or latest version*. A reference that names a revision such an
    /// object does not have is **left alone**: which half is wrong is not
    /// something the package says, the same rule the uuid follows.
    pub object_version: Option<String>,
}

/// Bring one object's references back in line with what the package holds.
///
/// # What is repaired, and on whose authority
///
/// A `DataObjectReference` caches four things about its target: the uuid, the
/// declared type, the title, and the revision. **The target object is the
/// authority for its own type, its own title and its own `objectVersion`** -
/// those are copies, and a copy that disagrees with the original is simply
/// stale. Those are rewritten.
///
/// The revision was added on 10/09, after `energyml check` learned to compare
/// it: a translated package stamps every object `objectVersion="0"` and wrote
/// every reference without one, so the package said two things and nothing
/// could put it right. A reference naming a revision the target does **not**
/// carry is left alone - clearing it would be deciding which half is wrong.
///
/// The target is *not* an authority for **which object was meant**. So a
/// reference naming a uuid that is not there, or naming an object of another
/// concept entirely, is reported and left exactly as it was: see [`Refusal`].
/// The rule this follows is that a repair may fix a copy, never a decision.
///
/// The uuid is never rewritten - it is the one field that says what was meant.
///
/// Only references that changed, or that were refused, come back; a reference
/// already in agreement produces nothing.
pub fn repair_references(
    source: &ObjectSummary,
    object: &mut dyn Reflect,
    target_of: &dyn Fn(&str) -> Option<Target>,
) -> Vec<RefRepair> {
    // The walk borrows the object, so the decisions are all taken first and the
    // writes happen after: what to write is settled before anything is written,
    // and a failure part-way cannot leave a reference half-updated.
    #[allow(clippy::type_complexity)]
    let mut planned: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = Vec::new();
    let mut out = Vec::new();

    for edge in identity::collect_refs(&*object) {
        let uuid = edge.target.uuid;
        if uuid.is_empty() {
            continue;
        }
        let declared = edge.target.qualified_type();
        let repair = |outcome| RefRepair {
            source: source.uri.clone(),
            path: edge.path.clone(),
            target_uuid: uuid.to_string(),
            outcome,
        };

        let Some(target) = target_of(uuid) else {
            out.push(repair(RepairOutcome::Refused(Refusal::Dangling)));
            continue;
        };
        if !same_concept(&declared, &target.qualified_type) {
            out.push(repair(RepairOutcome::Refused(Refusal::ConceptMismatch {
                declared,
                found: target.qualified_type,
            })));
            continue;
        }

        let type_drifted = !same_qualified_type(&declared, &target.qualified_type);
        let title_drifted = match (&target.title, edge.target.title) {
            (Some(theirs), Some(ours)) => theirs != ours,
            (Some(_), None) => true,
            (None, _) => false,
        };
        // Same shape as the title, with one difference that is the whole
        // point: a target carrying no revision leaves the reference as it is.
        let version_drifted = match (&target.object_version, edge.target.object_version) {
            (Some(theirs), Some(ours)) => theirs != ours,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if !type_drifted && !title_drifted && !version_drifted {
            continue;
        }
        planned.push((
            edge.path.clone(),
            target.qualified_type,
            title_drifted.then(|| target.title.clone()).flatten(),
            target.object_version.clone(),
            type_drifted.then(|| declared.clone()),
        ));
    }

    for (path, qualified_type, title, version, was_type) in planned {
        let reference = match energyml_core::path::object_mut(object, &path) {
            Ok(reference) => reference,
            // The `AnyXml` case, and anything else reflection cannot walk into.
            // Named rather than silently skipped: a repair that quietly does
            // nothing to some references is a repair nobody can trust.
            Err(e) => {
                out.push(RefRepair {
                    source: source.uri.clone(),
                    path,
                    target_uuid: String::new(),
                    outcome: RepairOutcome::Refused(Refusal::Unreachable {
                        reason: format!("{e}"),
                    }),
                });
                continue;
            }
        };
        let uuid = identity::uuid_of_reflect(&*reference)
            .unwrap_or_default()
            .to_string();
        let Ok(id) = identity::ObjectId::from_qualified_type(&qualified_type, &uuid) else {
            continue;
        };
        let mut key = id.as_key();
        key.title = title.as_deref();
        key.object_version = version.as_deref();
        if energyml_core::identity::set_target_reflect(reference, &key).is_ok() {
            out.push(RefRepair {
                source: source.uri.clone(),
                path,
                target_uuid: uuid,
                outcome: RepairOutcome::Done {
                    was_type,
                    now_type: qualified_type,
                    now_title: title,
                    now_version: version,
                },
            });
        }
    }
    out
}

/// Two qualified types naming the same type: `obj_` and case are noise.
fn same_qualified_type(a: &str, b: &str) -> bool {
    match (
        identity::parse_qualified_type(a),
        identity::parse_qualified_type(b),
    ) {
        (Ok(a), Ok(b)) => {
            a.domain.eq_ignore_ascii_case(b.domain)
                && a.version == b.version
                && naming::type_eq_loose(a.object_type, b.object_type)
        }
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// Whether a declared type names a concept the standard publishes in a
/// catalogue, so that not finding it in the package means nothing.
///
/// Matched on the type name alone, deliberately: `eml20` through `eml23` all
/// have `PropertyKind`, and a package may reference any of them.
pub fn is_externally_defined(qualified_type: &str) -> bool {
    let object_type = match identity::parse_qualified_type(qualified_type) {
        Ok(parsed) => parsed.object_type,
        // No domain prefix: take the whole string as the type name rather than
        // giving up, since a reference may declare only `PropertyKind`.
        Err(_) => qualified_type,
    };
    naming::type_eq_loose(object_type, "PropertyKind")
}

/// Same concept whatever the version: `resqml20.obj_Activity` and
/// `resqml22.Activity`.
fn same_concept(a: &str, b: &str) -> bool {
    match (
        identity::parse_qualified_type(a),
        identity::parse_qualified_type(b),
    ) {
        (Ok(a), Ok(b)) => {
            a.domain.eq_ignore_ascii_case(b.domain) && naming::type_eq_loose(a.object_type, b.object_type)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(uuid: &str, qualified_type: &str) -> ObjectSummary {
        ObjectSummary::new(
            format!("eml:///{qualified_type}({uuid})"),
            uuid.to_string(),
            qualified_type.to_string(),
        )
        .with_locator(format!("{qualified_type}_{uuid}.xml"))
    }

    #[test]
    fn the_obj_prefix_is_not_a_difference() {
        assert!(same_qualified_type("resqml20.obj_Activity", "resqml20.Activity"));
        // Case is noise on both halves - writers disagree about it, and a
        // reference is not wrong for having spelled the domain `RESQML`.
        assert!(same_qualified_type("RESQML20.obj_activity", "resqml20.Activity"));
        assert!(!same_qualified_type("resqml20.obj_Activity", "resqml22.Activity"));
        assert!(same_concept("resqml20.obj_Activity", "resqml22.Activity"));
        assert!(!same_concept("resqml20.obj_Activity", "resqml20.obj_Well"));
    }

    #[test]
    fn a_version_difference_is_not_a_concept_difference() {
        let target = summary("00000000-0000-4000-8000-000000000002", "resqml22.Activity");
        let same = summary("00000000-0000-4000-8000-000000000002", "resqml20.obj_Well");

        // Only the classification is exercised here; the walk over a real
        // object is covered by the corpus test, which has real DORs to walk.
        assert!(same_concept("resqml20.obj_Activity", &target.qualified_type));
        assert!(!same_concept("resqml20.obj_Activity", &same.qualified_type));
    }

    #[test]
    fn a_revision_is_compared_only_once_read_and_a_silent_reference_is_a_warning() {
        // To break: the absent-versus-absent case reported, `Unread` compared
        // as "carries none", or a reference naming a revision the object
        // lacks let through as a warning.
        let read = |version: Option<&str>| Cached::Read {
            object_version: version.map(str::to_string),
            title: None,
        };
        let revision = |referenced: Option<&str>, found: &Cached| revision_check(referenced, found, &[]);
        assert_eq!(revision(None, &Cached::Unread), None);
        assert_eq!(revision(Some("3"), &Cached::Unread), None);
        assert_eq!(revision(None, &read(None)), None);
        assert_eq!(revision(Some("3"), &read(Some("3"))), None);
        assert_eq!(
            revision(Some(" 3 "), &read(Some("3"))),
            None,
            "whitespace is not a revision"
        );

        let issue = |kind| RefIssue {
            source: "eml:///resqml22.HorizonInterpretation(a)".into(),
            path: "InterpretedFeature".into(),
            target_uuid: "b".into(),
            declared_type: None,
            kind,
        };
        let differ = revision(Some("2"), &read(Some("3"))).expect("a finding");
        assert_eq!(
            differ,
            RefIssueKind::ObjectVersionMismatch {
                referenced: Some("2".into()),
                found: Some("3".into()),
                candidates: vec![]
            }
        );
        assert!(issue(differ.clone()).is_error());
        assert!(issue(differ).to_string().contains("`2`"));

        let named_none = revision(Some("2"), &read(None)).expect("a finding");
        assert!(
            issue(named_none).is_error(),
            "a revision the object does not carry is promised"
        );

        // What our own translation used to write: objects at `0`, references
        // silent. The Identifier Specification lets a reader assume the
        // latest, so it is said and not counted as an error.
        let silent = revision(None, &read(Some("0"))).expect("a finding");
        assert_eq!(
            silent,
            RefIssueKind::ObjectVersionMismatch {
                referenced: None,
                found: Some("0".into()),
                candidates: vec![]
            }
        );
        assert!(!issue(silent.clone()).is_error());
        assert!(!issue(silent.clone()).is_info());
        assert!(issue(silent).to_string().contains("assumes the latest"));
    }

    #[test]
    fn a_title_is_a_copy_and_a_stale_one_is_said_without_being_an_error() {
        // A real case: the two references of a
        // `LocalEngineeringCompoundCrs` that our own translation wrote
        // carried an empty title, the Webstudio flagged it and this
        // checker did not look at titles at all. What it would take to
        // break this: comparing a title the target does not have, or making it
        // an error - a reader resolves by uuid and finds the right object.
        let read = |title: Option<&str>| Cached::Read {
            object_version: None,
            title: title.map(str::to_string),
        };
        assert_eq!(title_check(Some("The base"), &Cached::Unread), None);
        assert_eq!(
            title_check(Some("The base"), &read(None)),
            None,
            "no original, no stale copy"
        );
        assert_eq!(
            title_check(None, &read(Some("  "))),
            None,
            "a blank title is not a title"
        );
        assert_eq!(title_check(Some("The base"), &read(Some("The base"))), None);
        assert_eq!(
            title_check(Some(" The base "), &read(Some("The base"))),
            None,
            "whitespace is not a name"
        );

        let issue = |kind| RefIssue {
            source: "eml:///eml23.LocalEngineeringCompoundCrs(a)".into(),
            path: "VerticalCrs".into(),
            target_uuid: "b".into(),
            declared_type: None,
            kind,
        };
        let empty = title_check(None, &read(Some("Default local time CRS"))).expect("un constat");
        assert_eq!(
            empty,
            RefIssueKind::TitleMismatch {
                referenced: None,
                found: "Default local time CRS".into()
            }
        );
        assert!(
            !issue(empty.clone()).is_error(),
            "the uuid resolves: this is not an error"
        );
        assert!(!issue(empty.clone()).is_info());
        assert!(issue(empty).to_string().contains("no title"));

        let stale = title_check(Some("Ancien nom"), &read(Some("Nouveau nom"))).expect("un constat");
        assert!(issue(stale).to_string().contains("`Ancien nom`"));
    }

    #[test]
    fn a_property_kind_is_not_expected_to_be_in_the_package() {
        // Every eml version, and the bare name a reference may carry alone.
        for declared in [
            "eml20.PropertyKind",
            "eml23.PropertyKind",
            "EML23.propertykind",
            "PropertyKind",
        ] {
            assert!(is_externally_defined(declared), "{declared}");
        }
        // Nothing else gets the exemption - a missing representation is a
        // missing representation, and `PropertyKindFacet` is not a kind.
        for declared in [
            "resqml22.WellboreFrameRepresentation",
            "eml23.PropertyKindDictionary",
            "eml23.ColumnBasedTable",
        ] {
            assert!(!is_externally_defined(declared), "{declared}");
        }
    }

    #[test]
    fn an_absent_node_says_whether_its_absence_means_anything() {
        // The counterpart of `a_property_kind_is_not_expected_to_be_in_the
        // _package` one layer up: the predicate existed and the graph did not
        // carry it, so every viewer drawn from the graph counted a well-known
        // kind as a reference into nothing. `assemble` rather than `build`
        // because it is the path with no object to walk - the one an ETP client
        // takes - and because it needs no reflection here.
        let held = summary(
            "00000000-0000-4000-8000-000000000001",
            "resqml22.ContinuousProperty",
        );
        let kind = "eml:///eml23.PropertyKind(00000000-0000-4000-8000-000000000002)";
        let gone = "eml:///resqml22.HorizonInterpretation(00000000-0000-4000-8000-000000000003)";
        let graph = assemble(
            [&held],
            &|_| None,
            [
                Edge::new(held.uri.clone(), kind, "PropertyKind"),
                Edge::new(held.uri.clone(), gone, "Represents"),
            ],
        );

        let node = |uri: &str| graph.nodes.iter().find(|n| n.uri == uri).expect(uri);
        assert!(!node(kind).present && node(kind).externally_defined);
        // The one that must stay a fault: absent, and nothing publishes it.
        assert!(!node(gone).present && !node(gone).externally_defined);
        // A property of the type, not of the absence - so a consumer's rule is
        // the conjunction and never a flag read in a particular order.
        assert!(node(&held.uri).present && !node(&held.uri).externally_defined);
        assert!(graph.to_json().contains("\"externallyDefined\": true"));
    }

    #[test]
    fn a_catalogue_reference_is_neither_error_nor_warning() {
        let issue = |kind| RefIssue {
            source: "eml:///resqml22.ContinuousProperty(a)".into(),
            path: "PropertyKind".into(),
            target_uuid: "00000000-0000-4000-8000-000000000003".into(),
            declared_type: Some("eml23.PropertyKind".into()),
            kind,
        };
        let catalogue = issue(RefIssueKind::ExternallyDefined);
        assert!(!catalogue.is_error());
        assert!(catalogue.is_info());
        // The distinction the split exists for: a version mismatch is still a
        // warning about this package, and must not fall into the same bucket.
        let version = issue(RefIssueKind::VersionMismatch {
            found: "eml20.PropertyKind".into(),
            candidates: vec![],
        });
        assert!(!version.is_error());
        assert!(!version.is_info());
        // And a genuine dangling reference is still an error.
        assert!(issue(RefIssueKind::Dangling).is_error());
    }
}

// ---------------------------------------------------------------------------
// The `.rels` against the objects
// ---------------------------------------------------------------------------

/// A disagreement between the `.rels` index and the objects themselves.
///
/// The rule this crate follows everywhere - the `.rels` speed things up, the DORs
/// are authoritative - makes none of these fatal: whatever the index says, the reader
/// follows the `DataObjectReference`s and gets the right answer. They matter
/// for two other reasons.
///
/// A tool that **does** trust the index gets a different graph from the one the
/// data describes, and there are such tools. And the EPC specification §3.4.1
/// makes both directions mandatory - *"for every relationship between two XML
/// data objects there are two .rels files"* - so a disagreement is a departure
/// from the specification, which is exactly what `energyml check` is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelIssue {
    /// Part whose `.rels` this concerns.
    pub part: String,
    /// The other end: a part name, or a uuid when no part holds it.
    pub target: String,
    /// What is wrong.
    pub kind: RelIssueKind,
}

/// The four ways the index and the objects disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelIssueKind {
    /// The object references the target; no `destinationObject` says so.
    ///
    /// The common one, and the one that costs a viewer built on `.rels`: it
    /// draws a graph missing an edge the data plainly has.
    Undeclared {
        /// Where in the object the reference is.
        path: String,
    },
    /// A `destinationObject` declares a link the object does not make.
    ///
    /// The index remembers a reference that was edited away - an edge drawn
    /// from nothing.
    Unbacked {
        /// Relationship id, so it can be found in the file.
        id: String,
    },
    /// A relationship points at a part the package does not hold.
    Missing {
        /// Relationship id.
        id: String,
    },
    /// A `sourceObject` claims an object references this one, and it does not.
    ///
    /// The reverse direction §3.4.1 asks for, checked the same way.
    UnbackedSource {
        /// Relationship id.
        id: String,
    },
    /// The link exists, and the index has it the wrong way round.
    ///
    /// Reported as one finding rather than as the two it would otherwise
    /// produce - an unbacked relationship *and* an undeclared reference - since
    /// it is one mistake, and the producer's, not two of the reader's.
    /// `ALWYN-RESQML.epc` is full of them.
    Reversed {
        /// Relationship id.
        id: String,
        /// What it says now: `sourceObject` or `destinationObject`.
        declared: String,
    },
}

impl fmt::Display for RelIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            RelIssueKind::Undeclared { path } => write!(
                f,
                "{}: references {} at `{path}`, and no `destinationObject` relationship declares it",
                self.part, self.target
            ),
            RelIssueKind::Unbacked { id } => write!(
                f,
                "{}: `{id}` declares a link to {}, which the object itself does not reference",
                self.part, self.target
            ),
            RelIssueKind::Missing { id } => write!(
                f,
                "{}: `{id}` points at {}, which is not a part of this package",
                self.part, self.target
            ),
            RelIssueKind::UnbackedSource { id } => write!(
                f,
                "{}: `{id}` says {} references it, and it does not",
                self.part, self.target
            ),
            RelIssueKind::Reversed { id, declared } => write!(
                f,
                "{}: `{id}` is written `{declared}` and the link goes the other way",
                self.part
            ),
        }
    }
}

/// One object, with what its `.rels` declare and what its DORs actually say.
pub struct Related<'a> {
    /// The object's place in the inventory.
    pub summary: &'a ObjectSummary,
    /// The object itself, already deserialized.
    pub object: &'a dyn Reflect,
    /// The relationships recorded for its part.
    pub relationships: &'a [Relationship],
}

/// Confront the declared `.rels` with the references the objects really hold.
///
/// Both directions, because §3.4.1 asks for both and because they fail
/// differently: a link the objects make and the index omits leaves an
/// index-driven viewer short an edge; a link the index makes and the objects do
/// not leaves it drawing one from nothing.
///
/// A part name is matched on its **file name** rather than its path. A `.rels`
/// target is relative to the `_rels` folder's parent, so the same object is
/// written `Foo.xml` from the root and `../Foo.xml` from a subfolder - and an
/// EPC part name is unique across the package whichever layout it uses.
///
/// # Why this one *is* EPC-only, and stays so
///
/// Everything else in this module works from objects and would work over ETP
/// unchanged. This does not, and it should not be made to: it needs
/// [`crate::storage::LocalPackage::relationships`], and a `.rels` file is an
/// OPC artefact that exists nowhere else. There is nothing to confront in a
/// dataspace - the server *is* the index, so it cannot disagree with itself.
///
/// The reader-independent question - "what references what" - is
/// [`crate::storage::EnergymlStorage::edges`], and that one an ETP client
/// answers natively. This is the narrower question "does the package's own
/// index agree with its objects", and it is a question only a package has.
pub fn check_relationships(objects: &[Related<'_>]) -> Vec<RelIssue> {
    let leaf = |part: &str| -> String {
        part.rsplit(['/', '\\'])
            .next()
            .unwrap_or(part)
            .to_ascii_lowercase()
    };

    // Part file name → uuid, for turning a relationship target into an object.
    let mut by_part: std::collections::HashMap<String, &ObjectSummary> = std::collections::HashMap::new();
    for item in objects {
        by_part.insert(leaf(&item.summary.locator), item.summary);
    }
    // Uuid → object, for the undeclared references below. It was a linear
    // `find` over every object **per reference**: 108 s on a package of
    // 77 090 objects and 319 244 references), invisible on the corpus.
    // The first object holding a uuid wins, as `find` did.
    let mut by_uuid: std::collections::HashMap<String, &ObjectSummary> = std::collections::HashMap::new();
    for item in objects {
        by_uuid
            .entry(item.summary.uuid.to_ascii_lowercase())
            .or_insert(item.summary);
    }
    // A reference to a nested object is a link to the part that holds it -
    // what the writer records (see [`nested_index`]).
    let nested = nested_index(objects.iter().map(|item| (item.summary, item.object)));
    // Who references whom, from the objects themselves.
    let mut references: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for item in objects {
        let mut edges: Vec<(String, String)> = identity::collect_refs(item.object)
            .into_iter()
            .filter(|edge| !edge.target.uuid.is_empty())
            .filter_map(|edge| {
                let uuid = edge.target.uuid.to_ascii_lowercase();
                match nested.get(&uuid) {
                    // Nested in this very object: no link between two parts.
                    Some((_, container, _)) if *container == item.summary.uuid.to_ascii_lowercase() => None,
                    Some((_, container, _)) => Some((container.clone(), edge.path.clone())),
                    None => Some((uuid, edge.path.clone())),
                }
            })
            .collect();
        edges.sort();
        edges.dedup_by(|a, b| a.0 == b.0);
        references.insert(item.summary.uuid.to_ascii_lowercase(), edges);
    }

    let mut out = Vec::new();
    for item in objects {
        let part = &item.summary.locator;
        let mine = references
            .get(&item.summary.uuid.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default();

        let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();
        for relationship in item.relationships {
            // An external relationship names a file, not a part: that is how an
            // HDF5 proxy is bound, and it has no DOR to agree with.
            if relationship.external {
                continue;
            }
            let kind = relationship.short_kind().to_string();
            // `mlToExternalPartProxy` **is** how a package declares a reference
            // to an HDF5 proxy - EPC §3.4.1 names it, and this library's own
            // writer emits it rather than the generic pair. Not counting it as a
            // declaration made every package this library saved complain about
            // a link it had itself written correctly: two components of the
            // house disagreeing about one fact, which is the shape the rules
            // forbid. Found by cutting a real package and running `check` on the
            // result - not by reading the code.
            //
            // Its reverse, `externalPartProxyToMl`, is the proxy's own side and
            // reads like `sourceObject`.
            let kind = match kind.as_str() {
                "mlToExternalPartProxy" => "destinationObject".to_string(),
                "externalPartProxyToMl" => "sourceObject".to_string(),
                _ => kind,
            };
            if kind != "destinationObject" && kind != "sourceObject" {
                continue;
            }
            let Some(target) = by_part.get(&leaf(&relationship.target)) else {
                out.push(RelIssue {
                    part: part.clone(),
                    target: relationship.target.clone(),
                    kind: RelIssueKind::Missing {
                        id: relationship.id.clone(),
                    },
                });
                continue;
            };
            let target_uuid = target.uuid.to_ascii_lowercase();
            let me = item.summary.uuid.to_ascii_lowercase();
            // Which way the link really goes, from the objects.
            let forward = mine.iter().any(|(uuid, _)| *uuid == target_uuid);
            let backward = references
                .get(&target_uuid)
                .is_some_and(|edges| edges.iter().any(|(uuid, _)| *uuid == me));

            if kind == "destinationObject" {
                declared.insert(target_uuid.clone());
                if !forward {
                    out.push(RelIssue {
                        part: part.clone(),
                        target: target.uri.clone(),
                        kind: if backward {
                            RelIssueKind::Reversed {
                                id: relationship.id.clone(),
                                declared: kind,
                            }
                        } else {
                            RelIssueKind::Unbacked {
                                id: relationship.id.clone(),
                            }
                        },
                    });
                }
            } else if !backward {
                // A `sourceObject` that is really this object's own reference
                // is the same mistake seen from the other side; and the entry
                // is what makes the forward link "declared", so the second
                // complaint below does not fire as well.
                if forward {
                    declared.insert(target_uuid.clone());
                }
                out.push(RelIssue {
                    part: part.clone(),
                    target: target.uri.clone(),
                    kind: if forward {
                        RelIssueKind::Reversed {
                            id: relationship.id.clone(),
                            declared: kind,
                        }
                    } else {
                        RelIssueKind::UnbackedSource {
                            id: relationship.id.clone(),
                        }
                    },
                });
            }
        }

        // The other direction: what the object references and the index omits.
        // Only for targets the package holds - a reference out of the package
        // cannot have a relationship, and `check_references` already reports it
        // as dangling.
        for (uuid, path) in &mine {
            if declared.contains(uuid) {
                continue;
            }
            // `uuid` is already lower-cased (see `references` above).
            let Some(target) = by_uuid.get(uuid) else {
                continue;
            };
            out.push(RelIssue {
                part: part.clone(),
                target: target.uri.clone(),
                kind: RelIssueKind::Undeclared { path: path.clone() },
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The graph, for something to draw
// ---------------------------------------------------------------------------

/// One object, as a viewer needs to label it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// ETP URI - the identity that means the same thing in an EPC and in a
    /// dataspace, which is why it is the node's key rather than the part name.
    pub uri: String,
    /// Uuid on its own, for a viewer that wants the short form.
    pub uuid: String,
    /// `resqml22.TriangulatedSetRepresentation`.
    pub qualified_type: String,
    /// The concept, identical across versions: `TriangulatedSetRepresentation`.
    ///
    /// What to colour or group by. Grouping on the qualified type splits one
    /// concept into as many colours as the package has versions of it.
    pub type_key: String,
    /// `Citation/Title`, when there is one.
    pub title: Option<String>,
    /// Whether the package actually holds this object.
    ///
    /// A referenced object that is not here still gets a node - a graph that
    /// silently drops the edges leading out of the package is a graph that
    /// says the package is self-contained when it is not.
    pub present: bool,
    /// Whether the **type** names a concept the standard publishes in a
    /// catalogue, so that not finding it in the package means nothing.
    ///
    /// A property of the type and not of this node's presence, deliberately:
    /// a consumer's rule is then the plain conjunction - *absent **and** not
    /// externally defined is a dangling reference* - rather than a flag whose
    /// meaning depends on reading another flag first.
    ///
    /// It exists because a count that lacked it was **wrong in the one
    /// direction that matters**: on a BRGM package every one of the four
    /// absent nodes was a well-known `PropertyKind` - `code`, `description`,
    /// `formation name`, `unknown` - reported as four references into nothing.
    /// That is the case the project rules name: a reference to a well-known
    /// kind must not be reported as dangling. [`check_all_references`] already
    /// made the distinction ([`RefIssueKind::ExternallyDefined`]); the graph
    /// did not, so every viewer built on it made the same mistake - and
    /// re-deriving the predicate on the far side of the JSON would be two
    /// readers of one fact, which is what this crate does not do.
    pub externally_defined: bool,
}

/// One reference, from the object holding it to the object it names.
///
/// Built with [`Edge::new`]; `#[non_exhaustive]`, so that what ETP adds to an
/// edge is not a breaking change for every caller.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Edge {
    /// URI of the object holding the reference.
    pub from: String,
    /// URI of the object it names.
    pub to: String,
    /// Where inside the source: `Grid2dPatch.SupportingRepresentation`.
    pub path: String,
    /// ETP `RelationshipKind`: whether the reference is primary or secondary -
    /// what `navigableEdges` filters on. `None` when the store does not say:
    /// an ETP server states it; a store deriving edges from the objects does
    /// not, until the rule is written once from the specification
    /// (`TODO(verify)`: ETP 1.2 §8, Discovery, and the relationship rules of the
    /// Energistics Identifier Specification). Never guessed.
    pub kind: Option<EdgeKind>,
}

impl Edge {
    /// A reference whose kind is not stated.
    pub fn new(from: impl Into<String>, to: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            path: path.into(),
            kind: None,
        }
    }

    /// With the kind the store states.
    pub fn with_kind(mut self, kind: EdgeKind) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// ETP `RelationshipKind`, for one edge (`Both` is a filter, not an edge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// A primary relationship: the objects one reads together.
    Primary,
    /// A secondary relationship: one followed only on request.
    Secondary,
}

/// The reference graph of a set of objects.
///
/// # Why this is a free function over objects rather than a method on a store
///
/// The requirement asks for a graph on the storage parent class (parent of
/// epc and etp) so that graphs can be generated in every case. The way to
/// get that is not to put it on the trait - a default method would still have
/// to read every object, and deciding when to read is the caller's business
/// and nobody else's. It is to build it from what any implementation can hand
/// over: a summary and a deserialized object. An `EpcStore` produces those; an
/// ETP client will produce the same, and this will not have changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// Every object, and every object referenced from one.
    pub nodes: Vec<Node>,
    /// Every reference.
    pub edges: Vec<Edge>,
}

/// One node of a graph, among the nodes of its uuid.
struct Held {
    at: usize,
    qualified_type: String,
    version: Option<String>,
}

/// The `objectVersion` an object states, as an identity: blank is none.
fn version_of(object: &dyn Reflect) -> Option<String> {
    identity::object_version_of_reflect(object)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

/// The nodes a reference names among the nodes of its uuid: by type when the
/// type matches something (*a wrong version is not a wrong type*), by version
/// when the reference carries one - a version nobody carries still names the one
/// candidate there is - and when it names no version of an object held in
/// several, the one that declares none if exactly one does, **else all of them**.
fn select(held: &[Held], qualified_type: &str, version: Option<&str>) -> Vec<usize> {
    let mut left: Vec<&Held> = held.iter().collect();
    if left.iter().any(|one| one.qualified_type == qualified_type) {
        left.retain(|one| one.qualified_type == qualified_type);
    }
    if let Some(wanted) = version {
        if left.iter().any(|one| one.version.as_deref() == Some(wanted)) {
            left.retain(|one| one.version.as_deref() == Some(wanted));
        }
    } else if left.len() > 1 {
        let bare: Vec<&&Held> = left.iter().filter(|one| one.version.is_none()).collect();
        if let [only] = bare[..] {
            return vec![only.at];
        }
    }
    left.iter().map(|one| one.at).collect()
}

/// Build the graph of what references what.
///
/// `objects` pairs each summary with the object already deserialized, the same
/// shape [`check_all_references`] takes.
pub fn build<'a>(objects: impl IntoIterator<Item = (&'a ObjectSummary, &'a dyn Reflect)>) -> Graph {
    let items: Vec<(&ObjectSummary, &dyn Reflect)> = objects.into_iter().collect();
    let mut nodes: Vec<Node> = Vec::with_capacity(items.len());
    // Lowercased uuid → its nodes. **A list, because an object is (uuid, type,
    // objectVersion)** and a package may hold a uuid under two versions or two
    // releases: a map that kept "the last" made the second shadow the first and
    // sent every reference to it.
    let mut index: std::collections::HashMap<String, Vec<Held>> = std::collections::HashMap::new();

    for (summary, object) in &items {
        let key = summary.uuid.to_ascii_lowercase();
        index.entry(key).or_default().push(Held {
            at: nodes.len(),
            qualified_type: summary.qualified_type.clone(),
            version: version_of(*object),
        });
        nodes.push(Node {
            uri: summary.uri.clone(),
            uuid: summary.uuid.clone(),
            qualified_type: summary.qualified_type.clone(),
            type_key: identity::parse_qualified_type(&summary.qualified_type)
                .map(|parts| naming::strip_obj_prefix(parts.object_type).to_string())
                .unwrap_or_else(|_| summary.qualified_type.clone()),
            title: title_of(*object),
            present: true,
            externally_defined: is_externally_defined(&summary.qualified_type),
        });
    }

    // **A reference to an object nested in a part is a link to that part** -
    // a PRODML flow test naming a WITSML `ChannelSet` held in a `Log`, or its
    // `Channel`s. On the Lutetia model the statistics kept showing
    // "references to an absent object" - the front end counts
    // a node that is not present, and these were nodes for objects the package
    // does hold, one level down. The rule `derived_graph` already follows (see
    // [`nested_index`]): an edge to the container, none within one object.
    let nested = nested_index(items.iter().copied());
    let mut edges = Vec::new();
    for (summary, object) in &items {
        for edge in identity::collect_refs(*object) {
            if edge.target.uuid.is_empty() {
                continue;
            }
            let key = edge.target.uuid.to_ascii_lowercase();
            if let Some((_, container, _)) = nested.get(&key) {
                if *container == summary.uuid.to_ascii_lowercase() {
                    continue;
                }
                if let Some(at) = index
                    .get(container)
                    .and_then(|held| held.first())
                    .map(|held| held.at)
                {
                    edges.push(Edge::new(
                        summary.uri.clone(),
                        nodes[at].uri.clone(),
                        edge.path.clone(),
                    ));
                    continue;
                }
            }
            let wanted_version = edge
                .target
                .object_version
                .map(str::trim)
                .filter(|version| !version.is_empty());
            let held = index.entry(key).or_default();
            if held.is_empty() {
                // Referenced and not in the package. It gets a node saying so
                // rather than an edge into nothing.
                let qualified_type = edge.target.qualified_type();
                held.push(Held {
                    at: nodes.len(),
                    qualified_type: qualified_type.clone(),
                    version: wanted_version.map(str::to_string),
                });
                nodes.push(Node {
                    uri: identifiers::EtpUri::object_versioned(
                        None,
                        &qualified_type,
                        edge.target.uuid,
                        wanted_version,
                    ),
                    uuid: edge.target.uuid.to_string(),
                    type_key: naming::strip_obj_prefix(edge.target.object_type).to_string(),
                    externally_defined: is_externally_defined(&qualified_type),
                    qualified_type,
                    title: edge.target.title.map(str::to_string),
                    present: false,
                });
            }
            // The object the reference names - and, where it names no version of
            // an object held in several, **each** of them: this graph is what a
            // closure and a "who depends on this" are drawn from, and a
            // dependency that is too wide costs a copy, one that is too narrow
            // breaks a package.
            for at in select(held, &edge.target.qualified_type(), wanted_version) {
                edges.push(Edge::new(
                    summary.uri.clone(),
                    nodes[at].uri.clone(),
                    edge.path.clone(),
                ));
            }
        }
    }
    edges.sort_by(|a, b| (&a.from, &a.to, &a.path).cmp(&(&b.from, &b.to, &b.path)));
    edges.dedup();
    Graph { nodes, edges }
}

/// Build the graph from an inventory and a set of edges obtained *elsewhere*.
///
/// The counterpart of [`build`] for a store that can answer "what references
/// what" without handing over the objects - which is exactly what ETP does:
/// `GetResources` with `includeEdges` replies `GetResourcesEdgesResponse`, a
/// list of source/target pairs, and no object XML crosses the wire at all.
///
/// [`build`] deserializes every object to find the same edges. Both produce a
/// [`Graph`]; which one a store uses is the store's business, and that is the
/// whole point of [`crate::storage::EnergymlStorage::graph`] being a provided
/// method over [`crate::storage::EnergymlStorage::edges`].
///
/// `titles` is looked up by URI and may be missing entries - a graph without
/// labels is still a graph, and forcing a header read to draw one would undo
/// the saving this function exists for.
///
/// Edges naming a URI the inventory does not hold still get a node, marked
/// `present: false`, for the reason [`build`] does the same: a graph that drops
/// the edges leading out of the store claims the store is self-contained.
pub fn assemble<'a>(
    inventory: impl IntoIterator<Item = &'a ObjectSummary>,
    titles: &dyn Fn(&str) -> Option<String>,
    edges: impl IntoIterator<Item = Edge>,
) -> Graph {
    let mut nodes: Vec<Node> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for summary in inventory {
        index.insert(summary.uri.clone(), nodes.len());
        nodes.push(Node {
            uri: summary.uri.clone(),
            uuid: summary.uuid.clone(),
            qualified_type: summary.qualified_type.clone(),
            type_key: identity::parse_qualified_type(&summary.qualified_type)
                .map(|parts| naming::strip_obj_prefix(parts.object_type).to_string())
                .unwrap_or_else(|_| summary.qualified_type.clone()),
            title: titles(&summary.uri),
            present: true,
            externally_defined: is_externally_defined(&summary.qualified_type),
        });
    }

    let mut collected: Vec<Edge> = Vec::new();
    for edge in edges {
        for uri in [&edge.from, &edge.to] {
            if index.contains_key(uri) {
                continue;
            }
            // Only ever the target in practice, but a store may report an edge
            // from an object it no longer holds and the graph should say so
            // rather than lose the edge.
            let parsed = identifiers::EtpUri::parse(uri).ok();
            let qualified_type = parsed
                .as_ref()
                .and_then(|u| u.qualified_type())
                .map(|q| q.to_string())
                .unwrap_or_default();
            let uuid = parsed.as_ref().and_then(|u| u.uuid.clone()).unwrap_or_default();
            index.insert(uri.clone(), nodes.len());
            nodes.push(Node {
                uri: uri.clone(),
                uuid,
                type_key: identity::parse_qualified_type(&qualified_type)
                    .map(|parts| naming::strip_obj_prefix(parts.object_type).to_string())
                    .unwrap_or_else(|_| qualified_type.clone()),
                externally_defined: is_externally_defined(&qualified_type),
                qualified_type,
                title: None,
                present: false,
            });
        }
        collected.push(edge);
    }
    collected.sort_by(|a, b| (&a.from, &a.to, &a.path).cmp(&(&b.from, &b.to, &b.path)));
    collected.dedup();
    Graph {
        nodes,
        edges: collected,
    }
}

/// One object pointing at another, resolved far enough to act on.
///
/// Not a uuid: what a caller needs before deleting or copying something is
/// **which** object points at it and **where inside it** - so an interface can
/// take a person there, and a copy can follow the field rather than guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    /// URI of the object holding the reference.
    pub uri: String,
    /// Its uuid on its own.
    pub uuid: String,
    /// Its qualified type.
    pub qualified_type: String,
    /// Its citation title, when it has one.
    pub title: Option<String>,
    /// Where inside it the reference sits: `Grid2dPatch.SupportingRepresentation`.
    pub path: String,
}

/// What this object points at.
///
/// The cheap direction: an object carries its own references, so this reads one
/// object where [`dependents`] reads every one of them. The target's uri is
/// built exactly as [`build`] builds it - same crate, one spelling - and the
/// title is the **hint the reference carries**, which is not a source of truth:
/// what the target is actually called is a question for the package.
pub fn references(object: &dyn Reflect) -> Vec<Pointer> {
    identity::collect_refs(object)
        .into_iter()
        .filter(|edge| !edge.target.uuid.is_empty())
        .map(|edge| {
            let qualified_type = edge.target.qualified_type();
            Pointer {
                // The one constructor: the version the reference names, when it
                // names one, is part of the uri.
                uri: edge.target.etp_uri(None),
                uuid: edge.target.uuid.to_string(),
                qualified_type,
                title: edge.target.title.map(str::to_string),
                path: edge.path.clone(),
            }
        })
        .collect()
}

/// Who points at this uuid.
///
/// **The question to answer before deleting anything**, and the one direction a
/// package has no index for: a `DataObjectReference` says what it names, and
/// nothing says who named it. Every object has to be asked, which is why this
/// takes the objects rather than a store - the caller decides when to pay for
/// reading them, exactly as [`build`] does.
///
/// The comparison is on the uuid and case-insensitive, because that is what an
/// EPC writer varies and what the identifier specification says is the same
/// object.
pub fn dependents<'a>(
    objects: impl IntoIterator<Item = (&'a ObjectSummary, &'a dyn Reflect)>,
    uuid: &str,
) -> Vec<Pointer> {
    let wanted = uuid.to_ascii_lowercase();
    let mut out = Vec::new();
    for (summary, object) in objects {
        if summary.uuid.to_ascii_lowercase() == wanted {
            continue;
        }
        for edge in identity::collect_refs(object) {
            if edge.target.uuid.to_ascii_lowercase() != wanted {
                continue;
            }
            out.push(Pointer {
                uri: summary.uri.clone(),
                uuid: summary.uuid.clone(),
                qualified_type: summary.qualified_type.clone(),
                title: title_of(object),
                path: edge.path.clone(),
            });
        }
    }
    out
}

/// What a chosen set needs, and what it cannot get.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Closure {
    /// The seeds and everything they reach, in the graph's own order.
    ///
    /// The set [`crate::extract::plan`] would call closed - pass it as
    /// `wanted` and nothing breaks.
    pub needed: Vec<String>,
    /// Referenced, and the package does not hold it.
    ///
    /// **Named rather than dropped**: a closure that quietly ignored a
    /// reference into nothing would report a set as closed when the package it
    /// came from was already broken. Entries the standard publishes in a
    /// catalogue - a well-known `PropertyKind` - are not in here, because not
    /// holding them means nothing.
    pub missing: Vec<String>,
}

/// Everything the chosen objects need, following references as far as they go.
///
/// # Why this is a walk of its own
///
/// [`crate::extract::plan`] deliberately does **not** close a set: it takes the
/// set the caller decided on and reports what would break, because two
/// implementations of one walk would be two answers to the same question. This
/// is that one walk, in the library, and `plan` still checks the result - so a
/// caller can close a set here and be told by a different piece of code whether
/// it worked.
///
/// Seeds the graph does not hold are ignored rather than invented; `plan`
/// reports them as `unknown`, which is where that news belongs.
pub fn closure(graph: &Graph, seeds: &[String]) -> Closure {
    let mut out_edges: std::collections::HashMap<&str, Vec<&Edge>> = std::collections::HashMap::new();
    for edge in &graph.edges {
        out_edges.entry(edge.from.as_str()).or_default().push(edge);
    }
    let node_of: std::collections::HashMap<&str, &Node> =
        graph.nodes.iter().map(|node| (node.uri.as_str(), node)).collect();

    let mut reached: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut missing: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    for seed in seeds {
        if node_of.get(seed.as_str()).is_some_and(|node| node.present) && reached.insert(seed.clone()) {
            stack.push(seed.clone());
        }
    }
    while let Some(here) = stack.pop() {
        for edge in out_edges.get(here.as_str()).into_iter().flatten() {
            match node_of.get(edge.to.as_str()) {
                Some(node) if node.present => {
                    if reached.insert(edge.to.clone()) {
                        stack.push(edge.to.clone());
                    }
                }
                // Absent, and the standard does not publish it: the package
                // names something it does not hold, and that is the caller's
                // news to have.
                Some(node) if !node.externally_defined && !missing.contains(&edge.to) => {
                    missing.push(edge.to.clone());
                }
                _ => {}
            }
        }
    }

    Closure {
        needed: graph
            .nodes
            .iter()
            .filter(|node| reached.contains(&node.uri))
            .map(|node| node.uri.clone())
            .collect(),
        missing,
    }
}

/// An object's `Citation.Title`, the title a graph node carries - shared with
/// the package index (`energyml_domain::package_index`) so the two say the same.
pub fn title_of(object: &dyn Reflect) -> Option<String> {
    let energyml_core::value::FieldValue::Object(citation) = object.field("Citation")? else {
        return None;
    };
    match citation.field("Title")? {
        energyml_core::value::FieldValue::Scalar(scalar) => {
            scalar.as_str().map(str::to_string).filter(|t| !t.is_empty())
        }
        _ => None,
    }
}

impl Graph {
    /// The graph as JSON: `{"nodes":[…],"edges":[…]}`.
    ///
    /// Written by hand rather than derived, for the reason the rest of this
    /// workspace writes its own serializers: the shape is a **contract with a
    /// viewer**, and a derive would let a field rename in Rust silently rename
    /// a key that somebody's JavaScript reads.
    ///
    /// Keys: `uri`, `uuid`, `type` (the qualified type), `concept` (the
    /// version-independent one, for colouring), `title`, `present`,
    /// `externallyDefined`; and on an edge `from`, `to`, `path`.
    pub fn to_json(&self) -> String {
        let string = |value: &str| -> String {
            let mut out = String::with_capacity(value.len() + 2);
            out.push('"');
            for c in value.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        };

        let mut out = String::from("{\n  \"nodes\": [\n");
        for (n, node) in self.nodes.iter().enumerate() {
            out.push_str("    {");
            out.push_str(&format!("\"uri\": {}, ", string(&node.uri)));
            out.push_str(&format!("\"uuid\": {}, ", string(&node.uuid)));
            out.push_str(&format!("\"type\": {}, ", string(&node.qualified_type)));
            out.push_str(&format!("\"concept\": {}, ", string(&node.type_key)));
            match &node.title {
                Some(title) => out.push_str(&format!("\"title\": {}, ", string(title))),
                None => out.push_str("\"title\": null, "),
            }
            out.push_str(&format!("\"present\": {}, ", node.present));
            out.push_str(&format!("\"externallyDefined\": {}", node.externally_defined));
            out.push('}');
            if n + 1 < self.nodes.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n  \"edges\": [\n");
        for (n, edge) in self.edges.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"from\": {}, \"to\": {}, \"path\": {}}}",
                string(&edge.from),
                string(&edge.to),
                string(&edge.path)
            ));
            if n + 1 < self.edges.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ]\n}\n");
        out
    }

    /// The graph as Graphviz DOT, for a look without writing a viewer first.
    ///
    /// An object the package does not hold is drawn dashed: an edge leaving the
    /// package is the thing worth seeing at a glance.
    pub fn to_dot(&self) -> String {
        let escape = |value: &str| value.replace('"', "\\\"");
        let mut out = String::from("digraph energyml {\n  rankdir=LR;\n  node [shape=box];\n");
        for node in &self.nodes {
            let label = match &node.title {
                Some(title) => format!("{}\\n{}", escape(title), escape(&node.type_key)),
                None => escape(&node.type_key),
            };
            let style = if node.present {
                String::new()
            } else {
                ", style=dashed".to_string()
            };
            // The node's **uri** is its id: it names one entry, where the uuid
            // names every version of it - two versions of an object would be
            // one box, and an edge to either would land on the same one.
            out.push_str(&format!(
                "  \"{}\" [label=\"{label}\"{style}];\n",
                escape(&node.uri)
            ));
        }
        for edge in &self.edges {
            // An edge already holds the uris of its two nodes: no text is cut.
            out.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                escape(&edge.from),
                escape(&edge.to),
                escape(&edge.path)
            ));
        }
        out.push_str("}\n");
        out
    }
}
