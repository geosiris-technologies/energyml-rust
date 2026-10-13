//! Where a collection came from — **named**, and nothing fetched.
//!
//! The use case: GeoJSON import should accept local files, but also external
//! URLs.
//!
//! # Why the name is not a detail
//!
//! An import reads things from the name of what it was given. `sigeol-units`
//! takes the **stage** from it — `GeologicUnit_Bartonien_pretty.json` →
//! *Bartonien* — and that stage decides top from base for every surface of the
//! package ([`crate::sigeol::stage_of_file_name`]). A URL whose name came back
//! as `download?id=42`, or as the whole address, would not merely look untidy:
//! the operation would ask for a stage it could have read, and somebody would
//! type the wrong one.
//!
//! So a URL is reduced to **the file it names** — the last path segment, with
//! the query string and the fragment removed and the percent-escapes undone.
//!
//! # And nothing is fetched here
//!
//! `energyml-build` has no I/O and keeps none (a parser and a vocabulary, no
//! storage, no I/O). What this module decides is what
//! a caller needs **before** fetching — whether the text is to be read off a
//! disk or off a network, and what the result will be called — so that the CLI
//! and the front end ask the same question of the same code and cannot come to two
//! answers about the same address.

/// Where an input is to be read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A path on the machine running this.
    File(String),
    /// An `http` or `https` address.
    Url(String),
}

impl Source {
    /// The address or path, as it was written.
    pub fn as_str(&self) -> &str {
        match self {
            Source::File(text) | Source::Url(text) => text,
        }
    }
}

/// What an input is, read off its text.
///
/// **Only `http` and `https`.** Not `file:`, which a path already covers and
/// which two platforms spell differently; not `ftp:` or anything else, which no
/// caller here can fetch — and a scheme accepted but not fetchable is a
/// refusal that arrives one step too late, naming the network instead of the
/// address. A Windows drive letter (`C:/data/x.json`) is a path, which is why the
/// test is on the two schemes and not on *contains a colon*.
pub fn source_of(input: &str) -> Source {
    let lower = input.trim_start().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Source::Url(input.trim().to_string());
    }
    Source::File(input.to_string())
}

/// The name a source is known by — the file it names.
///
/// For a path, its last component. For a URL, the last **path** segment: the
/// query and the fragment are dropped, and percent-escapes are undone so that
/// `GeologicUnit_Lut%C3%A9tien.json` comes back as `GeologicUnit_Lutétien.json`
/// — the accent matters, because the stage is compared without folding it
/// ([`crate::sigeol::Unit::side_for`]).
///
/// Falls back to the whole text when there is nothing better: an address ending
/// in a slash names no file, and answering an empty string would give an import
/// a nameless collection rather than one it can say something about.
pub fn name_of(source: &Source) -> String {
    match source {
        Source::File(path) => path
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or(path)
            .to_string(),
        Source::Url(url) => {
            // The order is the point: strip the fragment, then the query — a
            // fragment may contain a `?`, and cutting the query first would
            // leave half of it behind.
            let without_fragment = url.split('#').next().unwrap_or(url);
            let path = without_fragment.split('?').next().unwrap_or(without_fragment);
            // **The authority is cut off by position, not by looking at it.**
            // Everything before the first `/` after `://` is the host and its
            // port — it names no file, and testing it for a colon instead let
            // `https://a.example/` answer *a.example*, which reads as a file
            // called after the server.
            let after_scheme = path.split_once("://").map(|(_, rest)| rest).unwrap_or(path);
            let Some((_, tail)) = after_scheme.split_once('/') else {
                // No path at all: the address itself is the only honest name,
                // and an import will say it read nothing from it.
                return url.clone();
            };
            match tail.rsplit('/').find(|part| !part.is_empty()) {
                Some(part) => unescape(part),
                None => url.clone(),
            }
        }
    }
}

/// Percent-escapes undone, bytes decoded as UTF-8.
///
/// Anything that does not decode is left **exactly as it was**: a name is for a
/// person to recognise, and a replacement character in the middle of one is
/// worse than the escape it replaced.
fn unescape(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            let pair = std::str::from_utf8(&bytes[at + 1..at + 3]).ok().and_then(|two| u8::from_str_radix(two, 16).ok());
            if let Some(byte) = pair {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_and_an_address_are_told_apart_by_their_scheme() {
        assert_eq!(source_of("data/x.geojson"), Source::File("data/x.geojson".into()));
        // A Windows path has a colon and is not an address — which is why the
        // test is on the scheme and not on *contains a colon*.
        assert_eq!(source_of("C:/data/x.json"), Source::File("C:/data/x.json".into()));
        assert_eq!(source_of("https://a.example/x.json"), Source::Url("https://a.example/x.json".into()));
        assert_eq!(source_of("HTTP://a.example/x.json"), Source::Url("HTTP://a.example/x.json".into()));
        // Not fetchable by any caller here, so it is a path and fails as one —
        // rather than being accepted and refused a step later by the network.
        assert!(matches!(source_of("ftp://a.example/x.json"), Source::File(_)));
        assert!(matches!(source_of("file:///c:/x.json"), Source::File(_)));
    }

    #[test]
    fn a_url_is_named_by_the_file_it_names_and_nothing_else() {
        // The whole reason this exists: `sigeol-units` reads the stage off the
        // name, and the stage decides top from base for every surface written.
        let named = |url: &str| name_of(&source_of(url));
        assert_eq!(named("https://a.example/d/GeologicUnit_Bartonien_pretty.json"), "GeologicUnit_Bartonien_pretty.json");
        assert_eq!(
            crate::sigeol::stage_of_file_name(&named("https://a.example/GeologicUnit_Bartonien_pretty.json")).as_deref(),
            Some("Bartonien")
        );
        // A token in the query would otherwise end up in the name — and in the
        // activity that records it.
        assert_eq!(named("https://a.example/GeologicUnit_Lutétien.json?token=secret"), "GeologicUnit_Lutétien.json");
        assert_eq!(named("https://a.example/x.json#part"), "x.json");
        // A fragment may hold a `?`: strip it first, or half of it stays.
        assert_eq!(named("https://a.example/x.json#a?b"), "x.json");
    }

    #[test]
    fn a_percent_escape_comes_back_as_the_letter_it_stood_for() {
        // The accent is not cosmetic: the stage is compared **without** folding
        // accents, so `Lutetien` and `Lutétien` are two stages.
        assert_eq!(
            name_of(&source_of("https://a.example/GeologicUnit_Lut%C3%A9tien_pretty.json")),
            "GeologicUnit_Lutétien_pretty.json"
        );
        // Something that is not an escape is left exactly as it was: a name is
        // for a person to recognise, and a replacement character is worse than
        // the `%` it replaced.
        assert_eq!(name_of(&source_of("https://a.example/100%_sure.json")), "100%_sure.json");
        assert_eq!(name_of(&source_of("https://a.example/a%zz.json")), "a%zz.json");
    }

    #[test]
    fn an_address_that_names_no_file_is_named_by_itself() {
        // Answering an empty string would give an import a nameless collection
        // instead of one it can say something about.
        assert_eq!(name_of(&source_of("https://a.example/")), "https://a.example/");
        assert_eq!(name_of(&source_of("https://a.example")), "https://a.example");
        // A port is part of the authority, not a file.
        assert_eq!(name_of(&source_of("https://a.example:8443")), "https://a.example:8443");
    }

    #[test]
    fn a_path_is_named_by_its_last_component_on_either_platform() {
        assert_eq!(name_of(&source_of("data/sub/x.geojson")), "x.geojson");
        assert_eq!(name_of(&source_of(r"C:\data\x.geojson")), "x.geojson");
        assert_eq!(name_of(&source_of("x.geojson")), "x.geojson");
        // A trailing separator names no file; the whole path is the answer.
        assert_eq!(name_of(&source_of("data/sub/")), "sub");
    }
}
