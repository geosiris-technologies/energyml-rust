//! Files that are not objects, and the objects they show.
//!
//! An EPC may carry "other files that contain additional, informal information
//! relevant to the contents of the package" - a PDF, a picture, a README -
//! "as a guideline … stored in a folder named `media`"
//! (`Energistics_Packaging_Conventions_v1.0_Doc_v1.2.pdf`, §3.2,
//! pp. 11-12; "An optional media folder may contain any type of media
//! file--no restrictions", §3.3 p. 14). Putting the bytes in is
//! [`crate::EpcStore::set_attachment`]; this module holds the rules that are
//! the specification's rather than the store's.
//!
//! # What ties a file to an object
//!
//! **Not the content type.** `[Content_Types].xml` gives each part its MIME
//! type (§2.2.1 p. 9, §3.3 p. 14) - it says *what* the part is, never *which
//! object* it is about. That is an OPC relationship, and §3.4.2 (pp. 15-16)
//! names the pair:
//!
//! - `destinationMedia` - "The object in Target is a media representation for
//!   the current object", in the **object's** `.rels`;
//! - `sourceMedia` - "The current object is a media representation for the
//!   object in Target", in the **part's** `.rels`.
//!
//! Both are written, as every other relationship of this writer is: §3.4.1
//! (p. 15) makes relationships to non-XML parts *optional* ("can be documented
//! but are optional", and "Rels files for other EPC file parts (jpg, pdf, etc.)
//! are optional"), but a link written one way cannot be walked back, which is
//! what a viewer showing an object's documents needs.
//!
//! # What is refused, and on whose authority
//!
//! - a part name OPC would not accept (§3.2.1 p. 12: "Parts in an EPC package
//!   must follow the OPC specification on part names"): an empty segment, a
//!   `.` or `..` segment, a segment ending in a dot, a backslash, a control
//!   character, a `%` escape of `/` or `\`;
//! - the reserved names of the package itself: `[Content_Types].xml`
//!   (§2.2.1 p. 9), a `_rels` folder and the `.rels` extension (§2.2.2 p. 9);
//! - a folder named like the ones EPC v1.1 reserves for data objects,
//!   `namespace_…` and `version_…`
//!   (`Energistics_Packaging_Conventions_v1.1_Doc_v1.0.pdf` §3.6 p. 17: other
//!   directories "MUST NOT start with any of the prefixes stated above");
//! - a content type that is not a media type - `type "/" subtype *(";"
//!   parameter)`, RFC 2616 §3.7. §3.3 (p. 14) names that grammar for the
//!   content type of an XML data object; §2.2.1 (p. 9) gives *every* part "a
//!   specific mime content type", and a MIME type has no other grammar, so the
//!   same rule is applied to a media part.
//!
//! Two part names that differ only by case are one part to OPC (ISO/IEC
//! 29500-2 §9.1.1.1, which EPC §3.2.1 defers to; that standard is not in this
//! repository - the rule is stated from it, not quoted), so adding the second
//! is a clash the store refuses by name.

use crate::error::{Error, Result};

/// `destinationMedia`, as EPC §3.4.2 (p. 16) spells it - in the object's `.rels`.
pub const DESTINATION_MEDIA: &str = "destinationMedia";

/// `sourceMedia`, as EPC §3.4.2 (p. 16) spells it - in the media part's `.rels`.
pub const SOURCE_MEDIA: &str = "sourceMedia";

/// A part of the package related to an object it is a media representation of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaLink {
    /// The part name, without a leading slash - `media/README.md`.
    pub part: String,
    /// The object's uuid, lowercased.
    pub uuid: String,
}

/// The part name a caller gave, normalised, or the rule it breaks.
///
/// The leading slash OPC writes is accepted and dropped: this store keys parts
/// the way the archive does.
pub fn check_part_name(part: &str) -> Result<String> {
    let name = part.trim_start_matches('/').to_string();
    let refuse = |why: &str| {
        Err(Error::Package(format!(
            "`{part}` is not a part name a package may hold: {why}"
        )))
    };
    if name.is_empty() || name.ends_with('/') {
        return refuse("it names a folder, not a file");
    }
    if name.contains('\\') {
        return refuse("a part name separates its segments with `/`, never `\\` (OPC, EPC §3.2.1 p. 12)");
    }
    if name.chars().any(char::is_control) {
        return refuse("it holds a control character");
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("%2f") || lower.contains("%5c") {
        return refuse("an escaped `/` or `\\` is not allowed in a part name (OPC, EPC §3.2.1 p. 12)");
    }
    for segment in name.split('/') {
        if segment.is_empty() {
            return refuse("it has an empty segment");
        }
        if segment == "." || segment == ".." {
            return refuse("`.` and `..` are not segments of a part name (OPC, EPC §3.2.1 p. 12)");
        }
        if segment.ends_with('.') {
            return refuse("a segment of a part name does not end with a dot (OPC, EPC §3.2.1 p. 12)");
        }
    }
    if name == "[Content_Types].xml" {
        return refuse("`[Content_Types].xml` is the package's index (EPC §2.2.1 p. 9), regenerated on save");
    }
    if lower.ends_with(".rels") || lower.split('/').any(|segment| segment == "_rels") {
        return refuse(
            "`_rels` and `.rels` are the package's relationships (EPC §2.2.2 p. 9), regenerated on save",
        );
    }
    let folders: Vec<&str> = lower.split('/').collect();
    if let Some(folder) = folders[..folders.len() - 1]
        .iter()
        .find(|folder| folder.starts_with("namespace_") || folder.starts_with("version_"))
    {
        return refuse(&format!(
            "a folder named `{folder}` is reserved for data objects (EPC v1.1 §3.6 p. 17: other directories \
             \"MUST NOT start with\" `namespace_` or `version_`)"
        ));
    }
    Ok(name)
}

/// Whether `text` is a media type, `type/subtype` with optional parameters -
/// the RFC 2616 §3.7 grammar (EPC §2.2.1 p. 9, §3.3 p. 14). `Ok` with the text
/// trimmed, or the refusal that says which half is wrong.
pub fn check_media_type(text: &str) -> Result<String> {
    let trimmed = text.trim();
    let refuse = |why: &str| {
        Err(Error::Package(format!(
            "`{text}` is not a content type: {why} - a media type is `type/subtype`, as RFC 2616 §3.7 \
             writes it (EPC §2.2.1 p. 9, §3.3 p. 14)"
        )))
    };
    let mut parts = trimmed.split(';');
    let head = parts.next().unwrap_or_default().trim();
    let Some((kind, subtype)) = head.split_once('/') else {
        return refuse("no `/` between a type and a subtype");
    };
    if !is_token(kind) {
        return refuse("the type is empty or holds a character a token may not");
    }
    if !is_token(subtype) {
        return refuse("the subtype is empty or holds a character a token may not");
    }
    for parameter in parts {
        let Some((attribute, value)) = parameter.trim().split_once('=') else {
            return refuse("a parameter is not `attribute=value`");
        };
        let value = value.trim();
        let quoted = value.len() >= 2 && value.starts_with('"') && value.ends_with('"');
        if !is_token(attribute.trim()) || !(quoted || is_token(value)) {
            return refuse("a parameter is not `attribute=value`");
        }
    }
    Ok(trimmed.to_string())
}

/// RFC 2616 §2.2 `token`: one or more characters that are neither controls nor
/// separators.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control() && !"()<>@,;:\\\"/[]?={} \t".contains(c))
}

/// Where a relationship's `Target` lands, as a part name - relative to the
/// folder of the part whose `.rels` holds it (OPC), or absolute with a leading
/// slash. `None` when it climbs out of the package.
pub fn resolve_target(from_part: &str, target: &str) -> Option<String> {
    let mut segments: Vec<&str> = if let Some(absolute) = target.strip_prefix('/') {
        return normalise(Vec::new(), absolute);
    } else {
        from_part.trim_start_matches('/').split('/').collect()
    };
    segments.pop();
    normalise(segments, target)
}

fn normalise<'a>(mut segments: Vec<&'a str>, path: &'a str) -> Option<String> {
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_part_under_media_is_a_part_name() {
        assert_eq!(check_part_name("/media/README.md").unwrap(), "media/README.md");
        assert_eq!(
            check_part_name("media/export/grille-craie.vtu").unwrap(),
            "media/export/grille-craie.vtu"
        );
    }

    #[test]
    fn what_opc_and_epc_reserve_is_refused() {
        for bad in [
            "",
            "media/",
            "media//x.pdf",
            "media/../x.pdf",
            "media/./x.pdf",
            "media\\x.pdf",
            "media/x.",
            "media%2Fx.pdf",
            "[Content_Types].xml",
            "media/_rels/x.pdf",
            "media/x.pdf.rels",
            "namespace_resqml22/x.pdf",
            "media/version_2/x.pdf",
        ] {
            assert!(check_part_name(bad).is_err(), "`{bad}` was accepted");
        }
        // A file *named* like a reserved folder is not in one.
        assert!(check_part_name("media/namespace_notes.md").is_ok());
    }

    #[test]
    fn a_content_type_is_a_media_type() {
        for good in [
            "text/markdown",
            "application/pdf",
            "model/obj",
            "application/geo+json",
            "text/markdown; charset=UTF-8",
            "application/x-resqml+xml;version=2.0;type=obj_Grid2dRepresentation",
            "text/plain; charset=\"utf-8\"",
        ] {
            assert!(check_media_type(good).is_ok(), "`{good}` was refused");
        }
        for bad in [
            "",
            "pdf",
            "text/",
            "/markdown",
            "text/mark down",
            "text/markdown; charset",
            "text/(md)",
        ] {
            assert!(check_media_type(bad).is_err(), "`{bad}` was accepted");
        }
    }

    #[test]
    fn a_target_resolves_against_the_folder_of_its_rels() {
        assert_eq!(
            resolve_target("Foo.xml", "media/README.md").as_deref(),
            Some("media/README.md")
        );
        assert_eq!(
            resolve_target("media/README.md", "../Foo.xml").as_deref(),
            Some("Foo.xml")
        );
        assert_eq!(
            resolve_target("media/export/a.obj", "../../resqml22/B.xml").as_deref(),
            Some("resqml22/B.xml")
        );
        assert_eq!(
            resolve_target("resqml22/B.xml", "/media/a.pdf").as_deref(),
            Some("media/a.pdf")
        );
        assert_eq!(resolve_target("Foo.xml", "../outside.pdf"), None);
    }
}
