//! Type naming: the `obj_` prefix, and how names are compared.
//!
//! Three names describe one generated type, and confusing them is an
//! interoperability bug rather than a cosmetic one:
//!
//! | name | `obj_Activity` | used by |
//! |---|---|---|
//! | [`TypeInfo::xml_name`] | `Activity` | the XML tag - the *element* name |
//! | [`TypeInfo::object_type`] | `obj_Activity` | contentType, qualifiedType, EPC part name - the *complexType* name |
//! | [`TypeInfo::type_key`] | `Activity` | grouping and translation across versions |
//!
//! RESQML 2.0.1 and common 2.0 prefix their top-level complexTypes with `obj_`
//! while naming the global element without it. An EPC written by fesapi carries
//! `type=obj_Activity` and names its part `obj_Activity_<uuid>.xml`, so the
//! prefix has to survive verbatim on the wire. Grouping the same concept across
//! versions, on the other hand, has to ignore it.
//!
//! The rule everywhere in this crate: **verbatim when writing, exact when
//! comparing, and loose only when the caller asks for it.**
//!
//! [`TypeInfo::xml_name`]: crate::reflect::TypeInfo::xml_name
//! [`TypeInfo::object_type`]: crate::reflect::TypeInfo::object_type
//! [`TypeInfo::type_key`]: crate::reflect::TypeInfo::type_key

/// The prefix RESQML 2.0.1 and common 2.0 put in front of top-level types.
pub const OBJ_PREFIX: &str = "obj_";

/// Remove a leading `obj_`, keeping the case of what follows.
///
/// Borrows: no allocation, and the result is a sub-slice of the input.
///
/// ```
/// # use energyml_core::naming::strip_obj_prefix;
/// assert_eq!(strip_obj_prefix("obj_Activity"), "Activity");
/// assert_eq!(strip_obj_prefix("Activity"), "Activity");
/// assert_eq!(strip_obj_prefix("Object"), "Object"); // not the prefix
/// ```
pub fn strip_obj_prefix(name: &str) -> &str {
    name.strip_prefix(OBJ_PREFIX).unwrap_or(name)
}

/// Whether a name carries the `obj_` prefix.
pub fn has_obj_prefix(name: &str) -> bool {
    name.starts_with(OBJ_PREFIX)
}

/// Exact comparison - case and prefix included. The default, and the one that
/// is right when the name came from a document.
pub fn type_eq(a: &str, b: &str) -> bool {
    a == b
}

/// Loose comparison: the `obj_` prefix is ignored on both sides, and so is
/// case. For user input and for grouping one concept across versions -
/// never for deciding what to write.
///
/// ```
/// # use energyml_core::naming::type_eq_loose;
/// assert!(type_eq_loose("obj_Activity", "activity"));
/// assert!(type_eq_loose("Activity", "obj_ACTIVITY"));
/// assert!(!type_eq_loose("Activity", "ActivityTemplate"));
/// ```
pub fn type_eq_loose(a: &str, b: &str) -> bool {
    strip_obj_prefix(a).eq_ignore_ascii_case(strip_obj_prefix(b))
}

/// Compact an Energistics version: `2.0` becomes `20`, as qualified types
/// write it.
pub fn compact_version(dotted: &str) -> String {
    dotted.chars().filter(|c| *c != '.').collect()
}

/// Expand a compact version: `20` becomes `2.0`, as content types write it.
///
/// Energistics writes versions without separators in qualified types and with
/// them in content types; the two have to be matched up.
pub fn dotted_version(compact: &str) -> String {
    let mut out = String::with_capacity(compact.len() * 2);
    for (i, c) in compact.chars().enumerate() {
        if i > 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Whether two versions denote the same schema version, whichever form each is
/// written in (`2.0` and `20`, `2.0.1` and `201`).
pub fn version_eq(a: &str, b: &str) -> bool {
    compact_version(a) == compact_version(b)
}

/// Split a version into its release and its pre-release suffix.
///
/// Energistics ships development drops beside the releases - `resqml
/// 2.2_dev3` sits next to `resqml 2.2` - and **the drop declares the release's
/// own version**: `v2.2_dev3/ResqmlAllObjects.xsd` says `version="2.2"`, the
/// same string the final 2.2 says. So a document written against dev3 is
/// indistinguishable from a 2.2 document by declaration alone; the directory
/// name is the only thing that separates them, and this is where that name is
/// taken apart.
///
/// Borrows: both halves are sub-slices of the input.
///
/// ```
/// # use energyml_core::naming::split_prerelease;
/// assert_eq!(split_prerelease("2.2"), ("2.2", None));
/// assert_eq!(split_prerelease("2.0.1"), ("2.0.1", None));
/// assert_eq!(split_prerelease("2.2_dev3"), ("2.2", Some("dev3")));
/// assert_eq!(split_prerelease("2.2dev3"), ("2.2", Some("dev3")));
/// // Leading junk is trimmed the way a declared `schemaVersion` needs:
/// // the 2016 Geosiris editor wrote `Resqml V2.0`.
/// assert_eq!(split_prerelease("V2.2-dev3"), ("2.2", Some("dev3")));
/// ```
pub fn split_prerelease(version: &str) -> (&str, Option<&str>) {
    let bare = version.trim();
    // A leading label is trimmed only when what follows still looks like a
    // dotted version. The 2016 Geosiris editor wrote `Resqml V2.0`, so the
    // trim has to exist; without the dot test it also eats the `dev` of a
    // bare `dev3` and reports the release `3`.
    let cut = bare.trim_start_matches(|c: char| !c.is_ascii_digit());
    let trimmed = if cut.contains('.') { cut } else { bare };
    match trimmed.find(|c: char| !c.is_ascii_digit() && c != '.') {
        None => (trimmed, None),
        Some(at) => {
            let release = trimmed[..at].trim_end_matches('.');
            let pre = trimmed[at..].trim_start_matches(['_', '-', ' ', '.']);
            (release, (!pre.is_empty()).then_some(pre))
        }
    }
}

/// The release half of a version: `2.2_dev3` is a `2.2`.
pub fn release_of(version: &str) -> &str {
    split_prerelease(version).0
}

/// Whether this version names a development drop rather than a release.
pub fn is_prerelease(version: &str) -> bool {
    split_prerelease(version).1.is_some()
}

/// Whether two pre-release suffixes name the same drop, whichever way each is
/// spelled: `dev3`, `DEV3` and `dev-3` are one.
///
/// The spelling varies because nothing standardises it - the directory says
/// `2.2_dev3`, a producer may write `2.2dev3` in `schemaVersion`, and matching
/// those as different strings would send a dev3 document to the wrong crate.
pub fn prerelease_eq(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_the_real_prefix() {
        assert_eq!(
            strip_obj_prefix("obj_EpcExternalPartReference"),
            "EpcExternalPartReference"
        );
        assert_eq!(strip_obj_prefix("object_Thing"), "object_Thing");
        assert_eq!(strip_obj_prefix("obj_"), "");
    }

    #[test]
    fn loose_comparison_ignores_case_and_prefix() {
        assert!(type_eq_loose(
            "obj_TriangulatedSetRepresentation",
            "triangulatedsetrepresentation"
        ));
        assert!(!type_eq("obj_Activity", "Activity"));
        assert!(type_eq("obj_Activity", "obj_Activity"));
    }

    #[test]
    fn versions_convert_both_ways() {
        assert_eq!(compact_version("2.0"), "20");
        assert_eq!(compact_version("2.0.1"), "201");
        assert_eq!(dotted_version("20"), "2.0");
        assert_eq!(dotted_version("201"), "2.0.1");
        assert!(version_eq("2.2", "22"));
        assert!(!version_eq("2.2", "20"));
    }

    #[test]
    fn a_release_has_no_prerelease_and_is_returned_whole() {
        // The existing versions must come through untouched: this function
        // runs on every candidate of every read, and a `2.0.1` that came back
        // as `2.0` plus a suffix would re-file every RESQML 2.0.1 object.
        for release in ["2.0", "2.1", "2.2", "2.3", "2.0.1"] {
            assert_eq!(split_prerelease(release), (release, None));
            assert_eq!(release_of(release), release);
            assert!(!is_prerelease(release));
        }
    }

    #[test]
    fn a_development_drop_splits_however_it_is_spelled() {
        // The directory spells it one way; nothing obliges a producer writing
        // `schemaVersion` to agree, so all four reach the same pair.
        for spelling in ["2.2_dev3", "2.2dev3", "2.2-dev3", "2.2 dev3"] {
            assert_eq!(
                split_prerelease(spelling),
                (
                    "2.2",
                    Some(spelling.trim_start_matches(['2', '.', '_', '-', ' ']))
                )
            );
            assert_eq!(release_of(spelling), "2.2");
            assert!(is_prerelease(spelling));
        }
    }

    #[test]
    fn leading_junk_is_trimmed_like_a_declared_version() {
        // `Resqml V2.0` is what the 2016 Geosiris editor wrote, and the
        // BRT packages proved a declaration this cannot read is worse than
        // none: every candidate ranks equal and "newest first" decides.
        assert_eq!(split_prerelease("Resqml V2.2_dev3"), ("2.2", Some("dev3")));
        assert_eq!(release_of("V2.0"), "2.0");
    }

    #[test]
    fn drops_are_compared_by_what_they_name_not_by_spelling() {
        assert!(prerelease_eq("dev3", "DEV3"));
        assert!(prerelease_eq("dev3", "dev-3"));
        assert!(!prerelease_eq("dev3", "dev2"));
        assert!(!prerelease_eq("dev3", "rc3"));
    }

    #[test]
    fn a_version_that_is_only_a_suffix_claims_no_release() {
        // Defensive: nothing should hand this in. What matters is that it
        // claims no release - a `dev3` that came back as the release `3`, or
        // as `2.2`, would file a document under a version nobody wrote.
        assert_eq!(split_prerelease("dev3"), ("", Some("dev3")));
        assert_eq!(release_of("dev3"), "");
    }
}
