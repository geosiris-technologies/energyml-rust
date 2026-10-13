//! A generic WFS 2.0.
//!
//! **The first shipped provider is not *BRGM*, it is *a WFS 2.0*.** That is what
//! makes adding the next service nearly free: an address, declared collections,
//! and the rest is here. Two INSPIRE services out of three will work without a
//! line more.
//!
//! # What was measured, and which no documentation says
//!
//! On `data.geoscience.fr/api/gsmlbh/ows`:
//!
//! - **`resultType=hits` returns the count without the data**: `numberMatched`,
//!   `numberReturned=0`, one kilobyte. It is the quote, and it is the **only**
//!   way to announce a cost before paying it, because in the full answer the
//!   counts are written **at the end** of the document;
//! - **a refusal arrives as an `ows:ExceptionReport`, in XML, with a 400**, even
//!   when `outputFormat=application/json` is requested;
//! - **the axis order depends on the parameter, not on the service.** This
//!   service wants `lat, lon` in a `bbox` naming `urn:ogc:def:crs:EPSG::4326`,
//!   and `lon, lat` in a CQL `SRID=4326;POLYGON`, which is WKT - measured,
//!   16,984 entities one way and **0 the other**, both with a 200 and no
//!   complaint. The first half had been measured and then **generalised to the
//!   second without being measured**, and `energyml fetch` returned zero on a
//!   URL that works in a browser. Hence [`Axes`], **declared** by the service,
//!   never guessed, and whose doc says *in which parameter*.

use energyml_build::geojson::{self, Feature, Flow, Summary, Wanted};

use crate::error::{Error, Result};
use crate::query::{Area, Collection, Page, Query, Term};
use crate::transport::{Answer, Request};
use crate::{About, Provider};

/// In which order this service wants the coordinates **in a CQL filter**.
///
/// Declared, never guessed, and the qualification "in a CQL filter" is the fruit
/// of a mistake: *the same service may want both*. Measured on the BRGM one, on
/// the same area -
///
/// | where | order | entities |
/// |---|---|---|
/// | `bbox=…,urn:ogc:def:crs:EPSG::4326` | `lat, lon` | boreholes around Paris |
/// | `cql_filter=Intersects(…,SRID=4326;POLYGON(…))` | **`lon, lat`** | **16,984** |
/// | the same CQL in `lat, lon` | - | **0** |
///
/// Both answer 200 without complaining, and one of the two returns zero
/// entities or those of another continent. That is why it is a field of the
/// provider, checked by a quote - which costs one kilobyte and would, here, have
/// avoided downloading fifty megabytes of nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axes {
    /// `lon, lat` - what CQL wants on the BRGM service, **measured**.
    LonLat,
    /// `lat, lon`, for a service that would ask for it that way.
    LatLon,
}

/// A WFS 2.0 service.
#[derive(Debug, Clone)]
pub struct Wfs {
    /// The short name.
    pub name: String,
    /// One sentence.
    pub about: String,
    /// The address of the `ows` entry point.
    pub endpoint: String,
    /// Who publishes, and under which licence.
    pub attribution: String,
    /// The licence, when known.
    pub licence: Option<String>,
    /// The CRS in which areas are sent, as it must be written.
    pub crs: String,
    /// The axis order this CRS imposes **for this service**.
    pub axes: Axes,
    /// What this service offers.
    pub collections: Vec<Collection>,
}

impl Wfs {
    /// The named collection, or a refusal that **lists what exists**.
    ///
    /// List rather than say "unknown": somebody who mistyped a name needs the
    /// names, not a finding.
    fn collection(&self, name: &str) -> Result<&Collection> {
        self.collections
            .iter()
            .find(|one| one.name == name)
            .ok_or_else(|| {
                Error::Refused(format!(
                    "`{name}` is not a collection of `{}` - it offers {}",
                    self.name,
                    self.collections
                        .iter()
                        .map(|one| one.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }

    /// The parameters common to every request on a collection.
    fn base(&self, collection: &Collection) -> String {
        let joiner = if self.endpoint.contains('?') { '&' } else { '?' };
        format!(
            "{}{joiner}service=WFS&version=2.0.0&request=GetFeature&typenames={}",
            self.endpoint,
            encode(&collection.feature_type)
        )
    }

    /// The CQL filter, when the request carries one.
    ///
    /// **Checked before being encoded**: a term that matches no declared filter
    /// is a refusal **here**, with no request - the server would refuse it too,
    /// but after a round trip and in its own words.
    fn cql(&self, collection: &Collection, query: &Query) -> Result<Option<String>> {
        let mut clauses: Vec<String> = Vec::new();

        if let Some(area) = &query.area {
            let Some(filter) = &collection.area else {
                return Err(Error::Refused(format!(
                    "`{}` cannot be asked for an area: it declares no geometry to test",
                    collection.name
                )));
            };
            clauses.push(format!(
                "Intersects(\"{}\",SRID=4326;{})",
                filter.property,
                self.wkt(area)
            ));
        }

        for term in &query.terms {
            let declared = self.declared(collection, term)?;
            clauses.push(format!("\"{}\"='{}'", declared.property, escape(&term.value)));
        }

        for filter in &collection.filters {
            if filter.required && !query.terms.iter().any(|term| term.name == filter.name) {
                return Err(Error::Refused(format!(
                    "`{}` of `{}` is required: {}",
                    filter.name, collection.name, filter.about
                )));
            }
        }

        Ok((!clauses.is_empty()).then(|| clauses.join(" and ")))
    }

    fn declared<'a>(&self, collection: &'a Collection, term: &Term) -> Result<&'a crate::query::Filter> {
        collection
            .filters
            .iter()
            .find(|one| one.name == term.name)
            .ok_or_else(|| {
                Error::Refused(format!(
                    "`{}` is not a filter of `{}` - it accepts {}",
                    term.name,
                    collection.name,
                    match collection.filters.is_empty() {
                        true => "none".to_string(),
                        false => collection
                            .filters
                            .iter()
                            .map(|one| one.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    }
                ))
            })
    }

    /// An area as WKT, **in the axis order this service imposes**.
    fn wkt(&self, area: &Area) -> String {
        let pair = |lon: f64, lat: f64| match self.axes {
            Axes::LonLat => format!("{lon} {lat}"),
            Axes::LatLon => format!("{lat} {lon}"),
        };
        match area {
            Area::Box {
                west,
                south,
                east,
                north,
            } => {
                let corners = [
                    (*west, *south),
                    (*west, *north),
                    (*east, *north),
                    (*east, *south),
                    (*west, *south),
                ];
                let points: Vec<String> = corners.iter().map(|(lon, lat)| pair(*lon, *lat)).collect();
                format!("POLYGON(({}))", points.join(","))
            }
            Area::Ring(points) => {
                let mut points = points.clone();
                // **Closing is a rule of the format, not a click.** A WKT ring
                // repeats its first point; requiring it from the caller would
                // make somebody click the same corner twice, which the map view
                // already settled for GeoJSON.
                if points.first() != points.last()
                    && let Some(first) = points.first().copied()
                {
                    points.push(first);
                }
                let points: Vec<String> = points.iter().map(|(lon, lat)| pair(*lon, *lat)).collect();
                format!("POLYGON(({}))", points.join(","))
            }
        }
    }
}

impl Provider for Wfs {
    fn name(&self) -> &str {
        &self.name
    }

    fn describe(&self) -> &str {
        &self.about
    }

    fn about(&self) -> About {
        About {
            endpoint: self.endpoint.clone(),
            attribution: self.attribution.clone(),
            licence: self.licence.clone(),
        }
    }

    fn collections(&self) -> Vec<Collection> {
        self.collections.clone()
    }

    fn count(&self, query: &Query) -> Result<Request> {
        let collection = self.collection(&query.collection)?;
        let mut url = self.base(collection);
        url.push_str("&resultType=hits");
        if let Some(cql) = self.cql(collection, query)? {
            url.push_str("&cql_filter=");
            url.push_str(&encode(&cql));
        }
        Ok(Request {
            url,
            // The quote answers in XML whatever is asked: announcing it avoids
            // suggesting that JSON was possible.
            accept: "application/xml".to_string(),
            about: format!("counting `{}` of `{}`", collection.name, self.name),
        })
    }

    fn page(&self, query: &Query, page: Page) -> Result<Request> {
        let collection = self.collection(&query.collection)?;
        let mut url = self.base(collection);
        url.push_str("&outputFormat=application%2Fjson");
        url.push_str(&format!("&count={}&startIndex={}", page.count, page.start));
        if let Some(cql) = self.cql(collection, query)? {
            url.push_str("&cql_filter=");
            url.push_str(&encode(&cql));
        }
        Ok(Request {
            url,
            accept: "application/geo+json, application/json;q=0.9".to_string(),
            about: format!(
                "`{}` of `{}`, {} from {}",
                collection.name, self.name, page.count, page.start
            ),
        })
    }

    fn read_count(&self, answer: Answer) -> Result<u64> {
        // The body of a quote is one kilobyte; reading it whole is the right
        // answer here, and the ceiling is only there for a faulty address.
        let status = answer.status;
        let text = answer.text(SMALL)?;
        if let Some(why) = exception(&text) {
            return Err(Error::Server {
                status,
                code: why.0,
                says: why.1,
            });
        }
        attribute(&text, "numberMatched")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| {
                Error::Unreadable(format!(
                    "this answer states no `numberMatched`: {}",
                    text.chars().take(200).collect::<String>()
                ))
            })
    }

    fn read_page(
        &self,
        answer: Answer,
        wanted: &Wanted<'_>,
        sink: &mut dyn FnMut(Feature) -> Flow,
    ) -> Result<Summary> {
        // **The refusal is read before the format.** A server that answers 400
        // does so in XML even when JSON is requested: deciding "this is not
        // JSON" would send somebody to look at their data instead of their
        // request.
        let status = answer.status;
        if status != 200 {
            let text = answer.text(SMALL)?;
            let (code, says) = exception(&text).unwrap_or((None, text));
            return Err(Error::Server { status, code, says });
        }
        geojson::read_streaming(answer.body, wanted, sink).map_err(|why| Error::Unreadable(why.to_string()))
    }
}

/// What a quote or a refusal is allowed to weigh.
///
/// An `ExceptionReport` is a few hundred bytes, a quote one kilobyte; beyond
/// that, it is no longer what one thinks one is reading, and refusing it is
/// better than putting a megabyte of HTML in an error message.
const SMALL: usize = 256 * 1024;

/// What an `ows:ExceptionReport` says: its code and its text.
///
/// Read as XML rather than as a string: an exception message carries entities
/// (`&quot;`), and gluing them back by hand is exactly the kind of detail that
/// makes a refusal unreadable at the moment it matters.
pub fn exception(text: &str) -> Option<(Option<String>, String)> {
    use quick_xml::events::Event;

    if !text.contains("ExceptionReport") {
        return None;
    }
    let mut reader = quick_xml::Reader::from_str(text);
    let mut code = None;
    let mut says = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                let raw = tag.name();
                let name = local(raw.as_ref());
                if name == "Exception" {
                    code = tag
                        .attributes()
                        .flatten()
                        .find(|a| local(a.key.as_ref()) == "exceptionCode")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                }
                if name == "ExceptionText" {
                    in_text = true;
                }
            }
            Ok(Event::Text(body)) if in_text => {
                if let Ok(decoded) = body.decode() {
                    says.push_str(&decoded);
                }
            }
            // **An entity is an event, not text.** quick-xml splits the content
            // around `&quot;` and returns the reference on its own; ignoring it
            // turned *Attribute "identifier" not found* into
            // `Attributeidentifiernot found` - a refusal one rereads twice
            // before understanding that it is ours.
            Ok(Event::GeneralRef(entity)) if in_text => match &*entity {
                b"quot" => says.push('"'),
                b"apos" => says.push('\''),
                b"amp" => says.push('&'),
                b"lt" => says.push('<'),
                b"gt" => says.push('>'),
                // An entity we cannot resolve is returned as it is: a truncated
                // message reads like a different message.
                other => {
                    says.push('&');
                    says.push_str(&String::from_utf8_lossy(other));
                    says.push(';');
                }
            },
            Ok(Event::End(tag)) => {
                let raw = tag.name();
                if local(raw.as_ref()) == "ExceptionText" {
                    in_text = false;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // Trimmed **once**, at the end: trimming each fragment ate the spaces that
    // border an entity, and that is exactly where they carry meaning.
    Some((code, says.trim().to_string()))
}

/// The value of an attribute of the first element that carries it.
fn attribute(text: &str, wanted: &str) -> Option<String> {
    let needle = format!("{wanted}=\"");
    let at = text.find(&needle)? + needle.len();
    let rest = &text[at..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn local(name: &[u8]) -> &str {
    let name = std::str::from_utf8(name).unwrap_or_default();
    name.rsplit(':').next().unwrap_or(name)
}

/// An apostrophe in a CQL value is doubled.
fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

/// Percent-encoding, done here rather than through one more dependency.
///
/// The list of characters **kept** is what RFC 3986 calls *unreserved*, nothing
/// more: everything else becomes `%XX`. A list of characters to escape would be
/// a list one forgets to complete, and a forgotten `&` cuts a request in two
/// without a word.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(*byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
