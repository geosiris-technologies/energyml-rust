// A reader for one known XML shape, not an XML parser.
//
// Two documents need it and neither justifies a dependency: the property kind
// dictionary (3 MB of the same five fields) and the RESQML graphical
// information that arrives inside an eml object as a raw subtree. Both have a
// fixed, shallow shape.
//
// Matching is on the **local name**, so `<eml:Title>`, `<Title>` and
// `<common:Title>` are the same element, and the five predefined entities are
// decoded. Nothing here validates: a caller checks that the document is the
// one it expects before trusting what comes out.
//
// (Plain `//` and not `//!`: `build.rs` includes this file verbatim, and an
// inner doc comment cannot appear in the middle of a file.)

// -- The scanning itself ----------------------------------------------------
//
// Matching is on the local name, so `<eml:Title>`, `<Title>` and
// `<common:Title>` are the same element. Nothing here tries to be an XML
// parser; it is a reader of one known document shape, and `scan` refuses
// anything that is not it.

pub fn is_name_boundary(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r' || byte == b'>' || byte == b'/'
}

/// Offset of the next `<[prefix:]name` opening tag.
pub fn find_open(text: &str, name: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(offset) = text[from..].find('<') {
        let at = from + offset;
        let rest = &text[at + 1..];
        let stripped = rest.strip_prefix(name).or_else(|| {
            let colon = rest.find(':')?;
            // A prefix must not contain a space or a `>`, or this is not a tag.
            if rest[..colon].bytes().any(is_name_boundary) {
                return None;
            }
            rest[colon + 1..].strip_prefix(name)
        });
        if let Some(after) = stripped
            && after.as_bytes().first().is_none_or(|b| is_name_boundary(*b))
            && !bytes[at..].starts_with(b"</")
        {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// Offset just past the matching `</[prefix:]name>`, searching from the start
/// of `text`, which must begin at the opening tag.
pub fn find_close(text: &str, name: &str) -> Option<usize> {
    let mut from = 1;
    while let Some(offset) = text[from..].find("</") {
        let at = from + offset;
        let rest = &text[at + 2..];
        let matched = rest.strip_prefix(name).or_else(|| {
            let colon = rest.find(':')?;
            if rest[..colon].bytes().any(is_name_boundary) {
                return None;
            }
            rest[colon + 1..].strip_prefix(name)
        });
        if let Some(after) = matched
            && let Some(end) = after.find('>')
            && after[..end].trim().is_empty()
        {
            return Some(at + 2 + name.len() + end + 1);
        }
        from = at + 2;
    }
    None
}

/// The text of the first `<name>…</name>` inside `text`.
pub fn element_text(text: &str, name: &str) -> Option<String> {
    let start = find_open(text, name)?;
    let after = &text[start..];
    let open_end = after.find('>')?;
    if after.as_bytes().get(open_end.wrapping_sub(1)) == Some(&b'/') {
        // `<Title/>`: present and empty.
        return Some(String::new());
    }
    let close = find_close(after, name)?;
    let inner = &after[open_end + 1..close.checked_sub(name.len() + 3)?];
    Some(decode(inner.trim()))
}

/// The whole `<name>…</name>` block, for reading a field inside it.
pub fn section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = find_open(text, name)?;
    let after = &text[start..];
    let close = find_close(after, name)?;
    Some(&after[..close])
}

/// The value of an attribute on the first tag of `text`.
pub fn attribute(text: &str, name: &str) -> Option<String> {
    let tag_end = text.find('>')?;
    let tag = &text[..tag_end];
    let mut from = 0;
    while let Some(offset) = tag[from..].find(name) {
        let at = from + offset;
        let before_is_space = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let after = tag[at + name.len()..].trim_start();
        if before_is_space && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let end = value[1..].find(quote)?;
                return Some(decode(&value[1..1 + end]));
            }
        }
        from = at + name.len();
    }
    None
}

pub fn contains_element(text: &str, name: &str) -> bool {
    find_open(text, name).is_some()
}

pub fn root_element(text: &str) -> Option<String> {
    let mut from = 0;
    while let Some(offset) = text[from..].find('<') {
        let at = from + offset;
        let rest = &text[at + 1..];
        if rest.starts_with('?') || rest.starts_with('!') {
            from = at + 1;
            continue;
        }
        let end = rest.find(|c: char| is_name_boundary(c as u8))?;
        return Some(rest[..end].to_string());
    }
    None
}

/// The five predefined entities, which are all a dictionary uses.
pub fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}


/// Every `<name>…</name>` block inside `text`, in document order.
///
/// The repeated form of [`section`]: a graphical information entry carries one
/// `IndexableElementInfo` per element kind, and reading only the first would
/// silently drop the colour of every other.
pub fn sections_all<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = find_open(rest, name) {
        let after = &rest[start..];
        let Some(end) = find_close(after, name) else {
            // A self-closing `<Foo/>` has no end tag: take just the tag.
            if let Some(tag_end) = after.find('>') {
                out.push(&after[..=tag_end]);
                rest = &after[tag_end + 1..];
                continue;
            }
            break;
        };
        out.push(&after[..end]);
        rest = &after[end..];
    }
    out
}

/// The text of every `<name>…</name>` inside `text`.
pub fn element_texts(text: &str, name: &str) -> Vec<String> {
    sections_all(text, name)
        .into_iter()
        .filter_map(|section| element_text(section, name))
        .collect()
}

/// The value of an attribute on the first tag, matched on its local name.
///
/// `xsi:type` and `type` are the same attribute here, because the prefix is
/// the document's business and not the reader's.
pub fn attribute_local(text: &str, local: &str) -> Option<String> {
    attribute(text, local).or_else(|| {
        let tag_end = text.find('>')?;
        let tag = &text[..tag_end];
        for (at, _) in tag.match_indices(':') {
            let after = &tag[at + 1..];
            if after.starts_with(local) {
                return attribute(&text[at + 1..], local);
            }
        }
        None
    })
}
