//! Writing an EPC package.
//!
//! **Saving is not editing an archive; it is rebuilding one.** An EPC carries
//! several parts that *describe* the others — `[Content_Types].xml`, every
//! `.rels`, `docProps/core.xml` — and a described state that no longer holds is
//! worse than no description at all. Patching them is how a package ends up
//! declaring an object it no longer contains, or an object twice, which is
//! exactly what the reference package published with fesapi does.
//!
//! So: everything descriptive is recomputed from the objects in memory, and
//! nothing is copied from the source archive except the bytes of attachments.
//!
//! # What the specification demands, and does not let you guess
//!
//! Three rules from `Energistics_Packaging_Conventions_v1.0_Doc_v1.2.pdf`
//! that a reasonable implementation gets wrong on the first try:
//!
//! - **Relationships go both ways** (§3.4.1): "for every relationship between
//!   two XML data objects there are two .rels files; one for the source object
//!   and one for the destination object". Writing only the source side produces
//!   a package that is not conformant, and that no consumer can walk backwards.
//! - **The core properties are mandatory** (§3.5.1), "contrary to the OPC
//!   standard", and `/_rels/.rels` must point at them (§3.5.2).
//! - **A part is named after the complexType**, not the element (§3.2.1):
//!   `obj_Activity_<uuid>.xml` in RESQML 2.0.1. [`ObjectKey::part_name`] already
//!   knows.
//!
//! # Everything must be in memory
//!
//! Regenerating the relationships means walking every object's DORs, so saving
//! forces the full read that [`crate::EpcStore`]'s lazy levels were designed to
//! avoid. That is not an oversight: laziness is a convenience for *looking at* a
//! package, never a way to write half of one.
//!
//! [`ObjectKey::part_name`]: energyml_core::identity::ObjectKey::part_name

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Seek, Write};

use energyml_core::identity::{self, EpcLayout};
use energyml_core::registry::{DynObject, to_part};
use energyml_core::xml::XmlWriteOptions;

use crate::error::{Diagnostic, Error, Result};

use super::core_props::{CORE_CONTENT_TYPE, CORE_PART, CORE_RELATIONSHIP, CoreProperties};
use super::package::rels_name_of;

/// Base of every Energistics relationship type (EPC §3.4.2).
const RELATIONSHIP_BASE: &str = "http://schemas.energistics.org/package/2012/relationships/";

/// Content type of a `.rels` part, fixed by OPC.
const RELS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// A non-EnergyML part carried along: a PDF, an image, a CSV — or an HDF5 file
/// stored inside the package.
pub struct Attachment {
    /// Part name, `media/report.pdf` by convention (§3.6 imposes nothing).
    pub part: String,
    /// Declared content type. Guessed from the extension when absent.
    pub content_type: Option<String>,
    /// The bytes, copied verbatim — the only thing that is.
    pub bytes: Vec<u8>,
}

/// Everything a package is made of, in memory.
///
/// A plain description rather than a store: an ETP client exporting a dataspace
/// to a file builds one of these without an [`crate::EpcStore`] existing
/// anywhere.
pub struct Package<'a> {
    /// Every EnergyML object the package will hold.
    pub objects: Vec<&'a dyn DynObject>,
    /// Non-EnergyML parts.
    pub attachments: Vec<Attachment>,
    /// `docProps/core.xml`.
    pub core: CoreProperties,
    /// Where the XML parts go. Flat is what most tools relax.
    pub layout: EpcLayout,
    /// Uuid of an `EpcExternalPartReference` → the file it stands for.
    ///
    /// The proxy object itself must not carry the name (§3.2.3: its `Filename`
    /// and `MimeType` "should be null"), so it is passed alongside and written
    /// into the proxy's `.rels`.
    pub external_files: BTreeMap<String, String>,
    /// Uuid of an **ordinary object** → the external file its own `.rels` named.
    ///
    /// # The relationship §3.2.3 does not describe, and producers write anyway
    ///
    /// The specification puts the file name in the proxy's `.rels` and nowhere
    /// else. Real packages disagree: `testingPackageCpp22.epc` puts an
    /// `externalResource` straight on **every object that holds an array** —
    /// 51 of them — and this reader has always honoured it (route 3 of
    /// `external_file_for`).
    ///
    /// The writer had no counterpart, so a rewrite dropped all 51. The package
    /// still read afterwards, because the search finds the file by other means,
    /// and that is exactly what made the loss invisible: another tool following
    /// the relationships as the package presents them would find nothing.
    ///
    /// Kept rather than derived, and only for the objects that had one. Adding
    /// the relationship to a package that never carried it would be this
    /// library inventing a convention, which is a different thing from not
    /// losing one.
    pub object_external_files: BTreeMap<String, String>,
    /// Parts kept exactly as they were, for what the specification does not
    /// describe — fesapi's `docProps/extendedCore.xml`, for instance. We never
    /// invent one; we do not throw one away either.
    pub verbatim: Vec<Attachment>,
    /// Which attachment is a media representation of which object (EPC §3.4.2
    /// pp. 15–16) — written `destinationMedia` in the object's `.rels` and
    /// `sourceMedia` in the part's (see [`super::media`]).
    ///
    /// A link to an object or a part the package does not hold is not written,
    /// and is reported: there is nothing to point at.
    pub media_links: Vec<super::media::MediaLink>,
}

impl<'a> Package<'a> {
    /// A package of these objects, with everything else empty.
    pub fn new(objects: Vec<&'a dyn DynObject>, core: CoreProperties) -> Self {
        Self {
            objects,
            attachments: Vec::new(),
            core,
            layout: EpcLayout::Flat,
            external_files: BTreeMap::new(),
            object_external_files: BTreeMap::new(),
            verbatim: Vec::new(),
            media_links: Vec::new(),
        }
    }
}

/// Write a package, reporting what could not be represented.
///
/// The diagnostics are not decoration: a reference to an object that is not in
/// the package cannot become a relationship — there is no part to point at —
/// and the caller has to be told rather than left with a quietly thinner
/// `.rels` than the one it had.
pub fn write_package<W: Write + Seek>(
    package: &Package<'_>,
    out: W,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    let plan = {
        let _span = tracing::info_span!("epc.write.plan", objects = package.objects.len()).entered();
        Plan::build(package, diagnostics)?
    };
    let mut zip = zip::ZipWriter::new(out);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let write_part = |zip: &mut zip::ZipWriter<W>, name: &str, bytes: &[u8]| -> Result<()> {
        zip.start_file(name, options)
            .map_err(|e| Error::Package(format!("writing `{name}`: {e}")))?;
        zip.write_all(bytes)?;
        Ok(())
    };

    write_part(&mut zip, "[Content_Types].xml", plan.content_types().as_bytes())?;

    {
        let _span = tracing::info_span!("epc.write.objects", parts = plan.objects.len()).entered();
        for (part, object) in plan.objects.iter() {
            let xml = to_part(*object, XmlWriteOptions::default())?;
            write_part(&mut zip, part, xml.as_bytes())?;
        }
    }
    for attachment in package.attachments.iter().chain(&package.verbatim) {
        write_part(&mut zip, &attachment.part, &attachment.bytes)?;
    }
    write_part(&mut zip, CORE_PART, package.core.to_xml().as_bytes())?;

    {
        let _span = tracing::info_span!("epc.write.rels", parts = plan.relationships.len()).entered();
        for (part, relationships) in &plan.relationships {
            write_part(&mut zip, part, render_relationships(relationships).as_bytes())?;
        }
    }

    zip.finish()
        .map_err(|e| Error::Package(format!("closing the archive: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The plan: every part name and every relationship, decided before a byte is
// written. Deciding while writing is how a `.rels` ends up describing a part
// that was named differently.
// ---------------------------------------------------------------------------

/// One relationship, as it will be written.
struct Rel {
    id: String,
    kind: &'static str,
    target: String,
    external: bool,
}

struct Plan<'a> {
    /// Part name → object, in the order they will be written.
    objects: Vec<(String, &'a dyn DynObject)>,
    /// `.rels` part name → its relationships.
    relationships: BTreeMap<String, Vec<Rel>>,
    /// Extension → content type, for the `Default` entries.
    defaults: BTreeMap<String, String>,
    /// Part name → content type, for the `Override` entries.
    overrides: BTreeMap<String, String>,
}

impl<'a> Plan<'a> {
    fn build(package: &'a Package<'a>, diagnostics: &mut Vec<Diagnostic>) -> Result<Self> {
        let mut plan = Plan {
            objects: Vec::new(),
            relationships: BTreeMap::new(),
            defaults: BTreeMap::from([("rels".to_string(), RELS_CONTENT_TYPE.to_string())]),
            overrides: BTreeMap::new(),
        };

        // 1 — where each object goes, and how the index will type it.
        //
        // **An object is (uuid, qualified type, objectVersion)**, so a uuid may
        // be held several times: under two versions, or under two releases. The
        // part names have to tell them apart (EPC v1.1 §3.6, p. 17: *« A data
        // object XML file MUST be contained in a unique path »*; *« If this data
        // object is versioned, the unique path should contain a directory called
        // "version_id" »*; *« If there is ambiguity about a data object's
        // namespace … the unique path can contain a directory called
        // "namespace_id" »*). So:
        //
        // - an object that shares its (uuid, type) with another **and declares a
        //   version** goes under `version_<id>/` — the one that declares none
        //   keeps the plain name, as a package without namesakes always had;
        // - an object whose name is still taken (the same uuid in two releases
        //   that name their types alike — `witsml20.Well` and `witsml21.Well`)
        //   goes under `namespace_<domain><version>/`;
        // - a name taken after both is a refusal, never an overwrite.
        let mut by_uuid: BTreeMap<String, Vec<Placed>> = BTreeMap::new();
        let mut proxy_uuids: BTreeSet<String> = BTreeSet::new();
        let mut taken: BTreeSet<String> = BTreeSet::new();
        let mut namesakes: BTreeMap<(String, String), usize> = BTreeMap::new();
        for object in &package.objects {
            if let Some(key) = identity::key_of_reflect(object.as_reflect()) {
                *namesakes.entry((key.uuid.to_ascii_lowercase(), key.qualified_type())).or_default() += 1;
            }
        }
        for object in &package.objects {
            let reflect = object.as_reflect();
            let key = identity::key_of_reflect(reflect).ok_or_else(|| {
                Error::Package(format!(
                    "`{}` carries no uuid, so it cannot be an EPC part",
                    reflect.type_info().name
                ))
            })?;
            let version = key.object_version.map(str::trim).filter(|version| !version.is_empty());
            let held_more_than_once = namesakes
                .get(&(key.uuid.to_ascii_lowercase(), key.qualified_type()))
                .is_some_and(|count| *count > 1);
            let mut part = key.part_path(package.layout);
            if held_more_than_once && let Some(version) = version {
                part = in_folder(&part, &format!("version_{}", encode_segment(version)));
            }
            if taken.contains(&part) {
                part = in_folder(&part, &format!("namespace_{}{}", key.domain, key.version.compact()));
            }
            if !taken.insert(part.clone()) {
                return Err(Error::Package(format!(
                    "two objects would be written to `{part}`: {} held twice under the same \
                     uuid, type and objectVersion",
                    key.etp_uri(None)
                )));
            }
            by_uuid.entry(key.uuid.to_ascii_lowercase()).or_default().push(Placed {
                part: part.clone(),
                qualified_type: key.qualified_type(),
                version: version.map(str::to_string),
            });
            if reflect.type_info().type_key == "EpcExternalPartReference" {
                proxy_uuids.insert(key.uuid.to_ascii_lowercase());
            }
            plan.overrides.insert(format!("/{part}"), key.content_type());
            plan.objects.push((part, *object));
        }
        // A data object nested in one of them — a WITSML `Channel` inside its
        // `ChannelSet` — is reached through the part that holds it
        // (`identity::nested_objects`). A part of its own wins: the nested
        // copy never displaces it.
        for (part, object) in &plan.objects {
            for nested in identity::nested_objects(object.as_reflect()) {
                by_uuid.entry(nested.key.uuid.to_ascii_lowercase()).or_insert_with(|| {
                    vec![Placed {
                        part: part.clone(),
                        qualified_type: nested.key.qualified_type(),
                        version: nested.key.object_version.map(str::to_string),
                    }]
                });
            }
        }

        // The placement is settled; the relationship pass reads it while
        // filling the map beside it, so it works from a copy.
        let placed = plan.objects.clone();

        // 2 — the relationships, both ways (§3.4.1).
        for (part, object) in &placed {
            let source_uuid = identity::uuid_of_reflect(object.as_reflect())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mut seen: BTreeSet<(String, &str)> = BTreeSet::new();

            for edge in identity::collect_refs(object.as_reflect()) {
                let target_uuid = edge.target.uuid.to_ascii_lowercase();
                if target_uuid.is_empty() {
                    continue;
                }
                let candidates = by_uuid.get(&target_uuid).map(Vec::as_slice).unwrap_or_default();
                let targets = pick(
                    candidates,
                    &edge.target.qualified_type(),
                    edge.target.object_version.map(str::trim).filter(|version| !version.is_empty()),
                );
                if targets.is_empty() {
                    // A dangling reference is not a relationship: there is no
                    // part to point at. Reported, never silently dropped.
                    diagnostics.push(Diagnostic::warning(
                        part.clone(),
                        format!(
                            "`{}` references {target_uuid}, which is not in this package; \
                             no relationship was written for it",
                            edge.path
                        ),
                    ));
                    continue;
                }
                // A reference that names no version of an object held in several
                // relates to **each** of them: a `.rels` is an index of the graph
                // (§3.4.1), and the graph draws that edge to every candidate —
                // too wide costs a link, too narrow a package that cannot be
                // walked.
                for target in targets {
                    let target_part = &target.part;
                    // Nested in this very part: no link between two parts.
                    if target_part == part {
                        continue;
                    }
                    // A proxy is reached by its own pair of types, not by the
                    // generic one (§3.4.2).
                    let (forward, backward) = if proxy_uuids.contains(&target_uuid) {
                        ("mlToExternalPartProxy", "externalPartProxyToMl")
                    } else {
                        ("destinationObject", "sourceObject")
                    };
                    // Two DORs to the same object are one relationship.
                    if !seen.insert((target_part.clone(), forward)) {
                        continue;
                    }
                    plan.add(part, forward, target_part, &id_tag(&by_uuid, &target_uuid, target_part));
                    plan.add(target_part, backward, part, &id_tag(&by_uuid, &source_uuid, part));
                }
            }
        }

        // 3 — each proxy names its file, in its `.rels` and nowhere else (§3.2.3).
        for (part, object) in &placed {
            let uuid = identity::uuid_of_reflect(object.as_reflect())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !proxy_uuids.contains(&uuid) {
                continue;
            }
            match package.external_files.get(&uuid) {
                Some(file) => plan
                    .relationships
                    .entry(rels_name_of(part))
                    .or_default()
                    .push(Rel {
                        // The specification recommends this exact id (§3.2.3).
                        id: "Hdf5File".to_string(),
                        kind: "externalResource",
                        target: file.clone(),
                        external: true,
                    }),
                None => diagnostics.push(Diagnostic::warning(
                    part.clone(),
                    "this external part reference names no file; the package will not say \
                     where its arrays live",
                )),
            }
        }

        // 3b — and the ones the *objects* carried themselves. Not in §3.2.3,
        // but written by real producers and honoured by this reader, so
        // dropping them on a rewrite loses information the package had.
        for (part, object) in &placed {
            let uuid = identity::uuid_of_reflect(object.as_reflect())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if proxy_uuids.contains(&uuid) {
                continue;
            }
            let Some(file) = package.object_external_files.get(&uuid) else {
                continue;
            };
            plan.relationships
                .entry(rels_name_of(part))
                .or_default()
                .push(Rel {
                    id: "Hdf5File".to_string(),
                    kind: "externalResource",
                    target: file.clone(),
                    external: true,
                });
        }

        // 4 — the package-level relationship to the core properties (§3.5.2).
        plan.relationships
            .entry("_rels/.rels".to_string())
            .or_default()
            .push(Rel {
                id: "CoreProperties".to_string(),
                kind: "",
                target: CORE_PART.to_string(),
                external: false,
            });
        plan.overrides
            .insert(format!("/{CORE_PART}"), CORE_CONTENT_TYPE.to_string());

        // 4b — the media parts and the objects they show, both ways (§3.4.2
        // pp. 15–16). Optional for a non-XML part (§3.4.1 p. 15), written
        // anyway: a link one way cannot be walked back.
        let carried: BTreeSet<&str> = package
            .attachments
            .iter()
            .chain(&package.verbatim)
            .map(|attachment| attachment.part.as_str())
            .collect();
        let mut linked: BTreeSet<(&str, &str)> = BTreeSet::new();
        for link in &package.media_links {
            let uuid = link.uuid.to_ascii_lowercase();
            let Some(object_part) = by_uuid.get(&uuid).and_then(|held| held.first()).map(|held| &held.part) else {
                diagnostics.push(Diagnostic::warning(
                    link.part.clone(),
                    format!(
                        "a media representation of {uuid}, which is not in this package; \
                         no relationship was written for it"
                    ),
                ));
                continue;
            };
            if !carried.contains(link.part.as_str()) {
                diagnostics.push(Diagnostic::warning(
                    object_part.clone(),
                    format!(
                        "`{}` is named as a media representation of this object and is not in the \
                         package; no relationship was written for it",
                        link.part
                    ),
                ));
                continue;
            }
            if !linked.insert((link.part.as_str(), link.uuid.as_str())) {
                continue;
            }
            plan.relationships
                .entry(rels_name_of(object_part))
                .or_default()
                .push(Rel {
                    id: media_id(&link.part),
                    kind: super::media::DESTINATION_MEDIA,
                    target: relative_target(object_part, &link.part),
                    external: false,
                });
            plan.relationships
                .entry(rels_name_of(&link.part))
                .or_default()
                .push(Rel {
                    id: format!("_{uuid}"),
                    kind: super::media::SOURCE_MEDIA,
                    target: relative_target(&link.part, object_part),
                    external: false,
                });
        }

        // 5 — the parts that are not EnergyML.
        for attachment in package.attachments.iter().chain(&package.verbatim) {
            match &attachment.content_type {
                Some(content_type) => {
                    plan.overrides
                        .insert(format!("/{}", attachment.part), content_type.clone());
                }
                None => {
                    let extension = extension_of(&attachment.part);
                    if let Some(guess) = content_type_for(&extension) {
                        plan.defaults
                            .entry(extension)
                            .or_insert_with(|| guess.to_string());
                    } else {
                        diagnostics.push(Diagnostic::note(
                            attachment.part.clone(),
                            "no content type and none guessable from the extension; \
                             the part is written but not typed",
                        ));
                    }
                }
            }
        }

        Ok(plan)
    }

    fn add(&mut self, part: &str, kind: &'static str, target: &str, target_uuid: &str) {
        self.relationships
            .entry(rels_name_of(part))
            .or_default()
            .push(Rel {
                // "It must start with a letter or an underscore (not a digit). It
                // is acceptable for the ID to always start with an underscore."
                // (§3.4.1) — a uuid starts with a digit often enough that the
                // underscore is not optional in practice.
                id: format!("_{target_uuid}"),
                kind,
                target: relative_target(part, target),
                external: false,
            });
    }

    fn content_types(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        out.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n");
        for (extension, content_type) in &self.defaults {
            out.push_str(&format!(
                "    <Default Extension=\"{}\" ContentType=\"{}\"/>\n",
                escape(extension),
                escape(content_type)
            ));
        }
        for (part, content_type) in &self.overrides {
            out.push_str(&format!(
                "    <Override PartName=\"{}\" ContentType=\"{}\"/>\n",
                escape(part),
                escape(content_type)
            ));
        }
        out.push_str("</Types>\n");
        out
    }
}

fn render_relationships(relationships: &[Rel]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    out.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n");
    // The ids of the relationships **before** this one, as they were stated —
    // a set rather than a scan of the list: a CRS named by sixty thousand
    // objects has a `.rels` of sixty thousand entries, and the scan was 70 %
    // of what saving a SIGEOL import cost (28/09). Same answer, same bytes.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::with_capacity(relationships.len());
    for (index, rel) in relationships.iter().enumerate() {
        let kind = if rel.kind.is_empty() {
            CORE_RELATIONSHIP.to_string()
        } else {
            format!("{RELATIONSHIP_BASE}{}", rel.kind)
        };
        // Ids must be unique inside one `.rels`; the same target can legitimately
        // appear under two different types.
        let id = if !seen.insert(rel.id.as_str()) {
            format!("{}_{index}", rel.id)
        } else {
            rel.id.clone()
        };
        out.push_str(&format!(
            "    <Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"",
            escape(&id),
            escape(&kind),
            escape(&rel.target)
        ));
        if rel.external {
            out.push_str(" TargetMode=\"External\"");
        }
        out.push_str("/>\n");
    }
    out.push_str("</Relationships>\n");
    out
}

/// A relationship target is relative to the folder holding the `.rels`, which
/// is the folder of the part it describes. Flat packages make this a no-op;
/// per-package folders do not.
fn relative_target(from_part: &str, to_part: &str) -> String {
    let from_dir = from_part.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let to_dir = to_part.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    if from_dir == to_dir {
        return to_part
            .rsplit_once('/')
            .map(|(_, file)| file.to_string())
            .unwrap_or_else(|| to_part.to_string());
    }
    // Up out of the source folder, then down into the target's. One level is
    // all the two permitted layouts ever need.
    let up = if from_dir.is_empty() {
        String::new()
    } else {
        "../".repeat(from_dir.matches('/').count() + 1)
    };
    format!("{up}{to_part}")
}

/// The id of a `destinationMedia` relationship: an XML id, so a letter or an
/// underscore first (§3.4.1 p. 15), and the rest of the part name reduced to
/// what an id may hold. Uniqueness inside one `.rels` is `render_relationships`'.
fn media_id(part: &str) -> String {
    let body: String = part
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    format!("_media_{body}")
}

fn extension_of(part: &str) -> String {
    part.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The few content types the specification names, plus the obvious ones.
fn content_type_for(extension: &str) -> Option<&'static str> {
    Some(match extension {
        // §3.2.3: an HDF5 file may live inside the package.
        "h5" | "hdf5" => "application/x-hdf5",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "json" => "application/json",
        "zip" => "application/zip",
        "segy" | "sgy" => "application/octet-stream",
        _ => return None,
    })
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// One uuid, several objects
// ---------------------------------------------------------------------------

/// Where an object of the package went: its part, and what tells it from the
/// other objects of its uuid.
struct Placed {
    part: String,
    qualified_type: String,
    version: Option<String>,
}

/// The objects a reference names, among those of its uuid — none when it is
/// dangling. The rules of `EpcStore::resolve_reference`, over the objects about
/// to be written: the type narrows when it matches something (*a wrong version
/// is not a wrong type*), the version when the reference carries one — a version
/// no object carries still names the one candidate there is — and a reference
/// that names no version takes the object that declares none if exactly one
/// does, **else all of them**.
fn pick<'a>(candidates: &'a [Placed], qualified_type: &str, version: Option<&str>) -> Vec<&'a Placed> {
    let mut held: Vec<&Placed> = candidates.iter().collect();
    if held.iter().any(|one| one.qualified_type == qualified_type) {
        held.retain(|one| one.qualified_type == qualified_type);
    }
    match version {
        Some(wanted) => {
            if held.iter().any(|one| one.version.as_deref() == Some(wanted)) {
                held.retain(|one| one.version.as_deref() == Some(wanted));
            }
        }
        None if held.len() > 1 => {
            let bare: Vec<&Placed> = held.iter().copied().filter(|one| one.version.is_none()).collect();
            if let [only] = bare[..] {
                return vec![only];
            }
        }
        None => {}
    }
    held
}

/// The part of a relationship's `Id` that names its target — the uuid, with the
/// target's rank among the objects of that uuid when there are several: an `Id`
/// is unique within a `.rels` (§3.4.1), and two targets of one uuid would
/// otherwise share it.
fn id_tag(by_uuid: &BTreeMap<String, Vec<Placed>>, uuid: &str, part: &str) -> String {
    match by_uuid.get(uuid) {
        Some(held) if held.len() > 1 => {
            let rank = held.iter().position(|one| one.part == part).unwrap_or(0);
            format!("{uuid}_{rank}")
        }
        _ => uuid.to_string(),
    }
}

/// `folder` inserted before the file name of a part path:
/// `obj_X_u.xml` → `folder/obj_X_u.xml`, `resqml20/obj_X_u.xml` →
/// `resqml20/folder/obj_X_u.xml`. EPC v1.1 §3.6 (p. 17): *« The order of these
/// directories does not matter »*.
fn in_folder(part: &str, folder: &str) -> String {
    match part.rsplit_once('/') {
        Some((parents, name)) => format!("{parents}/{folder}/{name}"),
        None => format!("{folder}/{part}"),
    }
}

/// A version as a part-name segment: *« If necessary, use URI encoding (%) to
/// escape special characters »* (EPC v1.1 §3.6, p. 17). Letters, digits and
/// `.`, `-`, `_` stay; every other byte of the UTF-8 becomes `%XX`.
fn encode_segment(version: &str) -> String {
    let mut out = String::with_capacity(version.len());
    for byte in version.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_relative_to_the_rels_folder() {
        assert_eq!(relative_target("Foo.xml", "Bar.xml"), "Bar.xml");
        assert_eq!(relative_target("resqml20/Foo.xml", "resqml20/Bar.xml"), "Bar.xml");
        assert_eq!(
            relative_target("resqml20/Foo.xml", "eml20/Bar.xml"),
            "../eml20/Bar.xml"
        );
        assert_eq!(relative_target("Foo.xml", "eml20/Bar.xml"), "eml20/Bar.xml");
    }

    #[test]
    fn an_external_relationship_says_so() {
        let rendered = render_relationships(&[Rel {
            id: "Hdf5File".to_string(),
            kind: "externalResource",
            target: "model.h5".to_string(),
            external: true,
        }]);
        assert!(rendered.contains("TargetMode=\"External\""));
        assert!(
            rendered.contains(
                "Type=\"http://schemas.energistics.org/package/2012/relationships/externalResource\""
            )
        );
    }

    #[test]
    fn ids_stay_unique_within_one_rels() {
        // The same target under two types: legal, and the ids must still differ.
        let uuid = "_2f975b89-2019-468f-91a0-f3f0b5c1638d";
        let rendered = render_relationships(&[
            Rel {
                id: uuid.to_string(),
                kind: "destinationObject",
                target: "A.xml".to_string(),
                external: false,
            },
            Rel {
                id: uuid.to_string(),
                kind: "sourceObject",
                target: "A.xml".to_string(),
                external: false,
            },
        ]);
        assert!(rendered.contains(&format!("Id=\"{uuid}\"")));
        assert!(rendered.contains(&format!("Id=\"{uuid}_1\"")), "{rendered}");
    }
}
