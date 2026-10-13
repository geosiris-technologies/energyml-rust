//! `docProps/core.xml` - the part that says who made the package and when.
//!
//! OPC treats the core properties as optional. **EPC does not**: §3.5.1 of the
//! packaging conventions says, in as many words, "Contrary to the OPC standard,
//! the Core Properties part is mandatory in an EPC file." So a package written
//! here always has one, created if the source had none.
//!
//! §3.5.3 fixes the contents, and the list is closed - "All other core
//! properties specified in OPC are assumed NOT to be given":
//!
//! | Field | | |
//! |---|---|---|
//! | `creator` | mandatory | free text |
//! | `created` | mandatory | the **package** creation time, W3CDTF |
//! | `version` | mandatory | the EPC specification version, `1.0` |
//! | `description`, `identifier`, `keywords`, `title` | optional | `identifier` is a `urn:uuid:…`, keywords are separated by `;` |
//!
//! `created` is the time the *package* was made, not the time its objects were:
//! it is rewritten on every save, and that is deliberate.

use std::fmt::Write as _;

use quick_xml::events::Event;

/// The EPC specification version this writer targets. There has been one.
pub const EPC_VERSION: &str = "1.0";

/// Part name of the core properties, at the location the specification
/// suggests (§3.5.2 leaves it free; every package in the wild uses this one).
pub const CORE_PART: &str = "docProps/core.xml";

/// Content type of the core properties part, fixed by OPC.
pub const CORE_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";

/// Relationship type pointing at the core properties, fixed by OPC and
/// required in `/_rels/.rels` by EPC §3.5.2.
pub const CORE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";

/// What `docProps/core.xml` carries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreProperties {
    /// Who made the package. Mandatory; free text.
    pub creator: String,
    /// When the package was made, in W3CDTF. Mandatory.
    pub created: String,
    /// EPC specification version. Mandatory, and it is [`EPC_VERSION`].
    pub version: String,
    /// Free text.
    pub description: Option<String>,
    /// `urn:uuid:…`.
    pub identifier: Option<String>,
    /// Separated by `;`, with no spaces around the separator (§3.5.3).
    pub keywords: Option<String>,
    /// Free text.
    pub title: Option<String>,
}

impl CoreProperties {
    /// Properties for a package being written now by `creator`.
    pub fn new(creator: impl Into<String>, created: impl Into<String>) -> Self {
        Self {
            creator: creator.into(),
            created: created.into(),
            version: EPC_VERSION.to_string(),
            ..Default::default()
        }
    }

    /// Read the part. Absent fields stay absent; nothing here fails, because a
    /// malformed `core.xml` must not stop a package from being read.
    pub fn parse(xml: &[u8]) -> Self {
        let mut out = CoreProperties::default();
        let mut reader = quick_xml::Reader::from_reader(xml);
        // Not `trim_text(true)`: since quick-xml 0.41 an entity is its own
        // event, so `Geosiris &amp; co` arrives in three pieces. Trimming each
        // one and taking the last would keep `co`. The run is accumulated and
        // trimmed once - the same mistake the XML reader made before the corpus
        // caught it.
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();
        let mut current = String::new();
        let mut text = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Err(_) | Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => {
                    current = local_name(e.name().as_ref());
                    text.clear();
                }
                Ok(Event::End(_)) => {
                    let value = std::mem::take(&mut text).trim().to_string();
                    if !value.is_empty() {
                        match current.as_str() {
                            "creator" => out.creator = value,
                            "created" => out.created = value,
                            "version" => out.version = value,
                            "description" => out.description = Some(value),
                            "identifier" => out.identifier = Some(value),
                            "keywords" => out.keywords = Some(value),
                            "title" => out.title = Some(value),
                            _ => {}
                        }
                    }
                    current.clear();
                }
                Ok(Event::Text(e)) => {
                    let raw = e.xml10_content().unwrap_or_default();
                    match quick_xml::escape::unescape(&raw) {
                        Ok(unescaped) => text.push_str(&unescaped),
                        Err(_) => text.push_str(&raw),
                    }
                }
                Ok(Event::CData(e)) => text.push_str(&String::from_utf8_lossy(&e)),
                Ok(Event::GeneralRef(e)) => {
                    if let Ok(name) = e.decode()
                        && let Ok(resolved) = quick_xml::escape::unescape(&format!("&{name};"))
                    {
                        text.push_str(&resolved);
                    }
                }
                _ => {}
            }
            buf.clear();
        }
        out
    }

    /// Write the part.
    ///
    /// Prefixes are spelled out rather than defaulted: `dcterms:` and `dc:` are
    /// two different vocabularies and three of these elements come from them,
    /// which is invisible in a document that hides them behind a default
    /// namespace.
    pub fn to_xml(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        out.push_str(
            "<coreProperties xmlns=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"\
             \n    xmlns:dcterms=\"http://purl.org/dc/terms/\"\
             \n    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\
             \n    xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n",
        );
        // The three mandatory ones, in the order the specification lists them.
        let _ = writeln!(
            out,
            "    <dcterms:created xsi:type=\"dcterms:W3CDTF\">{}</dcterms:created>",
            escape(&self.created)
        );
        let _ = writeln!(out, "    <dc:creator>{}</dc:creator>", escape(&self.creator));
        let version = if self.version.is_empty() {
            EPC_VERSION
        } else {
            &self.version
        };
        let _ = writeln!(out, "    <version>{}</version>", escape(version));

        for (tag, value) in [
            ("dc:title", &self.title),
            ("dc:description", &self.description),
            ("dc:identifier", &self.identifier),
            ("keywords", &self.keywords),
        ] {
            if let Some(value) = value.as_deref().filter(|v| !v.is_empty()) {
                let _ = writeln!(out, "    <{tag}>{}</{tag}>", escape(value));
            }
        }
        out.push_str("</coreProperties>\n");
        out
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn local_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw.rsplit(|b| *b == b':').next().unwrap_or_default()).into_owned()
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// Now, in the W3CDTF form §3.5.3 asks for: `2026-07-31T12:34:56Z`.
///
/// Computed here rather than pulled from a date library: one timestamp in one
/// format is not worth a dependency, and the civil-date arithmetic below is
/// Howard Hinnant's `civil_from_days`, which is exact for every date this will
/// ever see.
pub fn now_w3cdtf() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    w3cdtf(seconds)
}

/// A Unix timestamp in W3CDTF, UTC.
pub fn w3cdtf(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let secs_of_day = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day / 60) % 60,
        secs_of_day % 60
    )
}

/// A W3CDTF date-time in UTC - `2026-09-24T00:00:00Z` - to Unix seconds: the
/// inverse of [`w3cdtf`], for a caller that states the instant a package is
/// made (a recipe, so that two runs write the same `docProps/core.xml`).
///
/// Only the form [`w3cdtf`] writes is read - a `Z` offset, whole seconds;
/// anything else is `None`, never a guessed zone.
pub fn unix_seconds(w3cdtf: &str) -> Option<i64> {
    let text = w3cdtf.trim().strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date = date.splitn(3, '-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let mut time = time.splitn(3, ':');
    let hour: i64 = time.next()?.parse().ok()?;
    let minute: i64 = time.next()?.parse().ok()?;
    let second: i64 = time.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

/// A civil date to days since 1970-01-01 - the inverse of
/// [`civil_from_days`], Hinnant's algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days since 1970-01-01 to a civil date. Hinnant's algorithm, unchanged.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_time_read_back_is_the_one_written() {
        // Two independent sources: the formatter, and the date itself - the
        // Unix epoch and a leap day are the cases a sign or an era slip breaks.
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_seconds("2024-02-29T12:30:45Z"), Some(1_709_209_845));
        for seconds in [0, 951_782_400, 1_709_209_845, 1_790_208_000, -86_400] {
            assert_eq!(
                unix_seconds(&w3cdtf(seconds)),
                Some(seconds),
                "{}",
                w3cdtf(seconds)
            );
        }
        // No zone, a local offset, a nonsense month: not guessed.
        assert_eq!(unix_seconds("2026-09-24T00:00:00"), None);
        assert_eq!(unix_seconds("2026-09-24T00:00:00+02:00"), None);
        assert_eq!(unix_seconds("2026-13-24T00:00:00Z"), None);
    }

    #[test]
    fn timestamps_are_w3cdtf() {
        assert_eq!(w3cdtf(0), "1970-01-01T00:00:00Z");
        assert_eq!(w3cdtf(1_000_000_000), "2001-09-09T01:46:40Z");
        // A leap day, which is the whole reason not to hand-roll this casually.
        assert_eq!(w3cdtf(1_709_164_800), "2024-02-29T00:00:00Z");
        // Checked against Python's `datetime`, not against my arithmetic.
        assert_eq!(w3cdtf(1_785_000_000), "2026-07-25T17:20:00Z");
    }

    #[test]
    fn the_mandatory_three_survive_a_round_trip() {
        let props = CoreProperties {
            creator: "Geosiris & co".to_string(),
            created: "2026-07-31T12:00:00Z".to_string(),
            version: EPC_VERSION.to_string(),
            identifier: Some("urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6".to_string()),
            title: Some("Demo".to_string()),
            ..Default::default()
        };
        let back = CoreProperties::parse(props.to_xml().as_bytes());
        assert_eq!(back, props);
        assert!(props.to_xml().contains("Geosiris &amp; co"), "text is escaped");
    }

    #[test]
    fn a_real_core_xml_reads() {
        // Taken from the reference package published with fesapi.
        let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<coreProperties xmlns="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
 xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <dcterms:created xsi:type="dcterms:W3CDTF">2026-02-13T10:30:27Z</dcterms:created>
    <dc:creator>phili</dc:creator>
    <dc:identifier>urn:uuid:d93f926e-c4d8-471a-bb26-a9e0b0c7ac8c</dc:identifier>
    <version>1.0</version>
</coreProperties>"#;
        let props = CoreProperties::parse(xml);
        assert_eq!(props.creator, "phili");
        assert_eq!(props.created, "2026-02-13T10:30:27Z");
        assert_eq!(props.version, "1.0");
        assert_eq!(
            props.identifier.as_deref(),
            Some("urn:uuid:d93f926e-c4d8-471a-bb26-a9e0b0c7ac8c")
        );
    }
}
