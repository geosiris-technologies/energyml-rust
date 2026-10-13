// Scanning a `PropertyKindDictionary` document into flat entries.
//
// **Included verbatim by `build.rs`**, which is why it depends on nothing but
// `std`: the build script compacts the shipped dictionary with exactly the
// code that reads a dictionary the user supplies at runtime, so the two can
// never drift apart. Keep it self-contained.
//
// # Why not the real reader
//
// A dictionary is 3 662 objects in one document. Deserializing it through the
// registry would build 3 662 typed `PropertyKind`s to keep five fields of
// each, and would drag the eml 2.3 generated crate into `energyml-domain`,
// which is exactly the dependency this crate does not have.
//
// So this is a **narrow** scanner, and it says so: it reads the shape a
// dictionary has — a flat list of `PropertyKind` elements with a citation, an
// abstractness flag, a quantity class and a parent reference — and nothing
// else. Element names are matched on their **local name**, so any namespace
// prefix works, and the five predefined XML entities are decoded. A document
// that is not a dictionary is refused by name rather than read as empty.

// (Plain `//` and not `//!`: `build.rs` includes this file verbatim, and an inner doc
// comment cannot appear in the middle of a file.)

/// One kind, as the dictionary states it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawKind {
    /// Uuid, from the attribute on the element.
    pub uuid: String,
    /// `Citation.Title` — the name a person uses.
    pub title: String,
    /// Uuid of the parent kind, when it has one.
    ///
    /// The root of the dictionary is its own parent, which is why walking the
    /// chain must stop on a repeat rather than trusting it to end.
    pub parent: Option<String>,
    /// `QuantityClass` — the name of a class in the Energistics unit
    /// dictionary. Purely informational when it matches nothing there.
    pub quantity_class: Option<String>,
    /// An abstract kind should only ever be a parent (common TRG §11.9: *"should be used only as the parent of a real property"*).
    pub is_abstract: bool,
    /// Set when the dictionary marks the kind as no longer to be used.
    pub deprecated: Option<String>,
    /// `Citation.Description` — one sentence on what the kind means.
    ///
    /// Carried because a chooser over 3 662 entries is unusable without it:
    /// `elevation` and `measured depth` are both lengths, and only the sentence
    /// says that one is *height above a reference plane* and the other *depth
    /// along the borehole path*. Measured on the shipped dictionary: every kind
    /// has one, 310 kB in all, median 71 characters.
    pub description: Option<String>,
}

/// Read every `PropertyKind` of a dictionary document.
///
/// `Err` names what the document is instead of returning an empty list — a
/// silent empty dictionary would turn every later resolution into "not found",
/// which is the wrong diagnosis for "you loaded the wrong file".
pub fn scan(xml: &str) -> Result<Vec<RawKind>, String> {
    let body = xml.trim_start_matches('\u{feff}');
    if !contains_element(body, "PropertyKindDictionary") {
        return Err(match root_element(body) {
            Some(root) => format!(
                "this is a `{root}` document, not a `PropertyKindDictionary`"
            ),
            None => "this is not an XML document at all".to_string(),
        });
    }

    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = find_open(rest, "PropertyKind") {
        let after = &rest[start..];
        // The dictionary element itself opens with the same local name as its
        // children once the prefix is dropped, so the outer one is skipped by
        // requiring an end tag of the same name.
        let Some(end) = find_close(after, "PropertyKind") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end..];

        let Some(uuid) = attribute(block, "uuid") else {
            continue;
        };
        out.push(RawKind {
            uuid,
            title: element_text(block, "Title").unwrap_or_default(),
            parent: element_text(section(block, "Parent").unwrap_or(""), "Uuid"),
            quantity_class: element_text(block, "QuantityClass"),
            is_abstract: element_text(block, "IsAbstract")
                .is_some_and(|text| text.trim().eq_ignore_ascii_case("true")),
            deprecated: element_text(block, "DeprecationDate"),
            description: element_text(block, "Description"),
        });
    }

    if out.is_empty() {
        return Err("a `PropertyKindDictionary` that declares no property kind at all".to_string());
    }
    Ok(out)
}

/// The line form the build script writes and [`from_compact`] reads.
///
/// `allow(dead_code)`: only `build.rs` calls it, and `build.rs` includes this
/// file rather than importing it, so the crate's own compilation sees it
/// unused. Deleting it would break the build script.
#[allow(dead_code)]
///
/// Tab-separated, one kind per line, in the order the dictionary states them.
/// Chosen over a serialization format because it needs no dependency at build
/// time and none at run time, and because a person can read it.
pub fn to_compact(kinds: &[RawKind]) -> String {
    let mut out = String::new();
    for kind in kinds {
        let clean = |text: &str| text.replace(['\t', '\n', '\r'], " ");
        out.push_str(&clean(&kind.uuid));
        out.push('\t');
        out.push_str(&clean(&kind.title));
        out.push('\t');
        out.push_str(&clean(kind.parent.as_deref().unwrap_or("")));
        out.push('\t');
        out.push_str(&clean(kind.quantity_class.as_deref().unwrap_or("")));
        out.push('\t');
        out.push(if kind.is_abstract { '1' } else { '0' });
        out.push('\t');
        out.push_str(&clean(kind.deprecated.as_deref().unwrap_or("")));
        out.push('\t');
        out.push_str(&clean(kind.description.as_deref().unwrap_or("")));
        out.push('\n');
    }
    out
}

/// Read back what [`to_compact`] wrote.
pub fn from_compact(text: &str) -> Vec<RawKind> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let uuid = fields.next()?.to_string();
            let title = fields.next().unwrap_or("").to_string();
            let parent = non_empty(fields.next().unwrap_or(""));
            let quantity_class = non_empty(fields.next().unwrap_or(""));
            let is_abstract = fields.next().unwrap_or("0") == "1";
            let deprecated = non_empty(fields.next().unwrap_or(""));
            // **A field added after the fact reads as absent, not as a broken
            // line**: a line written by an earlier version has no description,
            // and rejecting the whole line would turn an older dictionary into
            // an empty one.
            let description = non_empty(fields.next().unwrap_or(""));
            Some(RawKind {
                uuid,
                title,
                parent,
                quantity_class,
                is_abstract,
                deprecated,
                description,
            })
        })
        .collect()
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<eml:PropertyKindDictionary schemaVersion="2.3" uuid="39ff6011-3451-4420-a877-ddc79b4024be">
  <eml:Citation><eml:Title>Property Kind Dictionary</eml:Title></eml:Citation>
  <eml:PropertyKind schemaVersion="2.3" uuid="a48c9c25-1e3a-43c8-be6a-044224cc69cb">
    <eml:Citation><eml:Title>property</eml:Title></eml:Citation>
    <eml:IsAbstract>true</eml:IsAbstract>
    <eml:QuantityClass>unitless</eml:QuantityClass>
    <eml:Parent>
      <eml:Uuid>a48c9c25-1e3a-43c8-be6a-044224cc69cb</eml:Uuid>
      <eml:Title>property</eml:Title>
    </eml:Parent>
  </eml:PropertyKind>
  <PropertyKind uuid="0f2a164d-cfd4-490b-b8dd-6d02fc98c20b">
    <Citation><Title>y gravity &amp; more</Title></Citation>
    <IsAbstract>false</IsAbstract>
    <QuantityClass>linear acceleration</QuantityClass>
    <Parent><Uuid>e8fe0e88-51ba-4d7d-bc91-d5b442d7287c</Uuid></Parent>
  </PropertyKind>
</eml:PropertyKindDictionary>"#;

    #[test]
    fn a_dictionary_reads_with_or_without_a_prefix() {
        let kinds = scan(SAMPLE).expect("it is a dictionary");
        assert_eq!(kinds.len(), 2, "{kinds:#?}");
        assert_eq!(kinds[0].title, "property");
        assert!(kinds[0].is_abstract);
        assert_eq!(kinds[0].quantity_class.as_deref(), Some("unitless"));
        // The root is its own parent — which is why the chain walk must stop
        // on a repeat rather than on an absent parent.
        assert_eq!(kinds[0].parent.as_deref(), Some(&kinds[0].uuid[..]));

        assert_eq!(kinds[1].title, "y gravity & more", "entities are decoded");
        assert!(!kinds[1].is_abstract);
        assert_eq!(
            kinds[1].parent.as_deref(),
            Some("e8fe0e88-51ba-4d7d-bc91-d5b442d7287c")
        );
    }

    #[test]
    fn the_wrong_document_is_named_rather_than_read_as_empty() {
        let error = scan("<?xml version=\"1.0\"?><eml:Well uuid=\"x\"><eml:Citation/></eml:Well>")
            .expect_err("a well is not a dictionary");
        assert!(error.contains("Well"), "{error}");
    }

    #[test]
    fn the_compact_form_round_trips() {
        let kinds = scan(SAMPLE).unwrap();
        assert_eq!(from_compact(&to_compact(&kinds)), kinds);
    }
}
