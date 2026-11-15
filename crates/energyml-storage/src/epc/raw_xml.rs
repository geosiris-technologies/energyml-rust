//! The two byte-level XML readings every part reader of this folder needs.
//!
//! Written four and three times over (one copy per reader) before they lived
//! here; the readers differ in what they look for, not in how a name or an
//! attribute value is read.

/// The local name of a possibly prefixed XML name: the part after the last colon.
pub(super) fn local_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw.rsplit(|b| *b == b':').next().unwrap_or_default()).into_owned()
}

/// A raw attribute value, unescaped. A value that does not unescape is kept as
/// written (lossy UTF-8), which is what a namespace uri or a content type
/// needs: reading what is there beats refusing a part over an `&`.
pub(super) fn unescape_attr(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    quick_xml::escape::unescape(&text)
        .map(|unescaped| unescaped.into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_is_dropped_and_an_entity_is_unescaped() {
        assert_eq!(local_name(b"dc:creator"), "creator");
        assert_eq!(local_name(b"creator"), "creator");
        assert_eq!(unescape_attr(b"a&amp;b"), "a&b");
        assert_eq!(unescape_attr(b"a & b"), "a & b");
        // Break it: make `local_name` split on the first colon instead of the last.
    }
}
