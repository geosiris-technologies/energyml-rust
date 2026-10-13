//! Reading a `FeatureCollection` — the format, and nothing about geology.
//!
//! # Why this is not the exporter run backwards
//!
//! [`energyml_export::geojson`] writes what this workspace knows: a feature per
//! object, `properties` carrying uuid, uri, type. Reading a **foreign** file is
//! the opposite problem — the producer's vocabulary is unknown, arbitrarily
//! nested, and the only honest thing to do with it is keep it. So this module
//! parses the *format* (RFC 7946) and stops there; what the properties mean is
//! [`crate::sigeol`]'s business, and neither knows about RESQML.
//!
//! # The guard, in the same direction as the exporter's refusal
//!
//! RFC 7946 removed the `crs` member in 2016 and made the format WGS 84 by
//! definition. Producers ignore this constantly: a "GeoJSON" from a French tool
//! very often carries Lambert-93, where a longitude of 650 000 is perfectly
//! well-formed JSON. Read without a check it places a Paris-basin model in the
//! Gulf of Guinea, silently.
//!
//! The exporter refuses to *write* eastings into a longitude field; this
//! refuses to *read* them, **naming the pair**, because the number is the
//! diagnosis: somebody who sees `650000, 6860000` recognises Lambert-93 without
//! being told what to look for. A legacy 2008 `crs` member that names anything
//! other than CRS84/4326 is refused for the same reason — it is the producer
//! saying, in the file, that the coordinates are not what the format promises.

use serde_json::Value;

/// A position as the format defines it: longitude, latitude, and an optional
/// third ordinate whose meaning the format does not fix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Degrees east.
    pub lon: f64,
    /// Degrees north.
    pub lat: f64,
    /// The third ordinate, when the file gives one. RFC 7946 calls it an
    /// elevation; a producer may mean anything by it, so nothing here does.
    pub z: Option<f64>,
}

/// What a geometry is, once its positions are grouped.
///
/// The same three the map reader settled on, and for the same reason: a hull of
/// nodes sharing one place degenerates, and a producer honestly writes a
/// `Point` rather than a ring of four identical corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A place, or several: `Point`, `MultiPoint`.
    Points,
    /// One or more open polylines.
    Lines,
    /// One or more closed rings.
    Areas,
}

/// One feature: what it is, where it is, and everything it says about itself.
#[derive(Debug, Clone)]
pub struct Feature {
    /// The feature's own `id`, when it has one. Traceability, never identity:
    /// RFC 7946 lets it be a string or a number and does not require it.
    pub id: Option<String>,
    /// What kind of geometry it carries.
    pub shape: Shape,
    /// Positions in parts — one part per ring, per line, per place.
    ///
    /// Kept as parts rather than flattened: joining two fault sticks end to end
    /// would draw a line the file does not state.
    pub parts: Vec<Vec<Position>>,
    /// `properties`, **verbatim**, nesting included.
    pub properties: Value,
}

impl Feature {
    /// Every position of every part, in order — for a caller that wants the
    /// cloud and not the topology.
    pub fn positions(&self) -> impl Iterator<Item = Position> + '_ {
        self.parts.iter().flat_map(|part| part.iter().copied())
    }
}

/// What a collection gave, and what it could not.
///
/// `refused` is never a count. A feature dropped without a reason is
/// indistinguishable from one the file never had — the rule the whole workspace
/// follows, and the reason a reader that refuses half a file still returns the
/// other half.
#[derive(Debug, Clone, Default)]
pub struct Collection {
    /// The features that read.
    pub features: Vec<Feature>,
    /// One sentence per feature that did not, naming it.
    pub refused: Vec<String>,
}

/// The whole document failed: it is not JSON, not a collection, or it declares
/// a coordinate reference system that is not WGS 84.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected(pub String);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Rejected {}

/// Read a `FeatureCollection`.
///
/// Fails as a whole only for what makes the document unusable; a feature that
/// cannot be read is named in `refused` and the rest still comes back.
/// **One implementation, two faces.** This is [`read_streaming`] with
/// everything kept and a sink that pushes into a `Vec` — so a caller holding a
/// whole document in memory and one reading a network answer as it arrives get
/// the same refusals, word for word, and the format is decided in one place.
pub fn read(text: &str) -> Result<Collection, Rejected> {
    let mut out = Collection::default();
    let summary = read_streaming(text.as_bytes(), &Wanted::default(), |one| {
        out.features.push(one);
        Flow::Continue
    })?;
    out.refused = summary.refused;
    Ok(out)
}

fn feature(entry: &Value, at: usize) -> Result<Feature, String> {
    let id = match entry.get("id") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    // Named by its id when it has one, by its rank when it has not: a refusal
    // saying "one feature was dropped" is a refusal nobody can act on.
    let name = id.clone().unwrap_or_else(|| format!("feature #{at}"));

    let geometry = entry
        .get("geometry")
        .ok_or_else(|| format!("{name}: no geometry"))?;
    if geometry.is_null() {
        // Legal in RFC 7946 §3.2, and useless here: an unlocated feature has
        // nothing to place. Said rather than skipped.
        return Err(format!("{name}: an unlocated feature (`geometry: null`)"));
    }
    let kind = geometry
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name}: the geometry has no type"))?;
    // **Before the `coordinates` check, and that ordering is the point.** A
    // `GeometryCollection` carries `geometries`, not `coordinates`, so demanding
    // coordinates first made this refusal unreachable and answered with *the
    // geometry has no coordinates* — true, useless, and it sends the reader
    // looking for a malformed file rather than for the one shape this reader
    // will not take. Found by the test written for the message.
    if kind == "GeometryCollection" {
        return Err(format!(
            "{name}: a GeometryCollection may hold a mixture, and one feature says one thing"
        ));
    }
    let coordinates = geometry
        .get("coordinates")
        .ok_or_else(|| format!("{name}: the geometry has no coordinates"))?;

    let (shape, parts) = match kind {
        "Point" => (Shape::Points, vec![vec![position(coordinates, &name)?]]),
        // One part per position, never one part holding many: two boreholes are
        // two places, and a single part would let a caller join them into a line.
        "MultiPoint" => (Shape::Points, each(coordinates, &name, |p, n| Ok(vec![position(p, n)?]))?),
        "LineString" => (Shape::Lines, vec![line(coordinates, &name)?]),
        "MultiLineString" => (Shape::Lines, each(coordinates, &name, line)?),
        // The outer ring only. A hole is a fact about the shape that nothing
        // downstream models, and reading it as a second ring would invent a
        // second shape.
        "Polygon" => (Shape::Areas, vec![outer_ring(coordinates, &name)?]),
        "MultiPolygon" => (Shape::Areas, each(coordinates, &name, outer_ring)?),
        other => return Err(format!("{name}: `{other}` is not a GeoJSON geometry")),
    };
    if parts.iter().all(|part| part.is_empty()) {
        return Err(format!("{name}: no position at all"));
    }
    Ok(Feature {
        id,
        shape,
        parts,
        properties: entry.get("properties").cloned().unwrap_or(Value::Null),
    })
}

fn each<F>(value: &Value, name: &str, mut one: F) -> Result<Vec<Vec<Position>>, String>
where
    F: FnMut(&Value, &str) -> Result<Vec<Position>, String>,
{
    let list = value
        .as_array()
        .ok_or_else(|| format!("{name}: the coordinates are not an array"))?;
    list.iter().map(|entry| one(entry, name)).collect()
}

fn line(value: &Value, name: &str) -> Result<Vec<Position>, String> {
    let list = value
        .as_array()
        .ok_or_else(|| format!("{name}: the coordinates are not an array"))?;
    list.iter().map(|p| position(p, name)).collect()
}

fn outer_ring(value: &Value, name: &str) -> Result<Vec<Position>, String> {
    let rings = value
        .as_array()
        .ok_or_else(|| format!("{name}: the coordinates are not an array of rings"))?;
    let outer = rings
        .first()
        .ok_or_else(|| format!("{name}: a polygon with no ring"))?;
    let positions = line(outer, name)?;
    // RFC 7946 §3.1.6. Kept as a check because a two-corner "polygon" draws as
    // a sliver, and a shape that is not what the file meant is worse than none.
    if positions.len() < 4 {
        return Err(format!(
            "{name}: a closed ring needs 4 positions, this one has {} (RFC 7946 §3.1.6)",
            positions.len()
        ));
    }
    Ok(positions)
}

fn position(value: &Value, name: &str) -> Result<Position, String> {
    let list = value
        .as_array()
        .ok_or_else(|| format!("{name}: a position is not an array"))?;
    let number = |at: usize| list.get(at).and_then(Value::as_f64);
    let (Some(lon), Some(lat)) = (number(0), number(1)) else {
        return Err(format!("{name}: a position with fewer than two numbers"));
    };
    if !lon.is_finite() || !lat.is_finite() || lon.abs() > 180.0 || lat.abs() > 90.0 {
        // The pair is in the message because the number *is* the diagnosis.
        return Err(format!(
            "{name}: [{lon}, {lat}] is not a longitude and a latitude — a GeoJSON is WGS 84 by \
             definition (RFC 7946), and these look like projected metres"
        ));
    }
    Ok(Position { lon, lat, z: number(2).filter(|v| v.is_finite()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(features: &str) -> String {
        format!("{{\"type\":\"FeatureCollection\",\"features\":[{features}]}}")
    }

    #[test]
    fn a_point_keeps_its_third_ordinate_and_its_properties() {
        // The BRGM shape: a 3D point whose interesting half is in `properties`.
        let text = collection(
            r#"{"type":"Feature","id":"525007",
                "geometry":{"type":"Point","coordinates":[2.2953,48.875,58]},
                "properties":{"@featureType":"GeologicUnit"}}"#,
        );
        let read = read(&text).expect("a collection");
        assert!(read.refused.is_empty());
        let feature = &read.features[0];
        assert_eq!(feature.id.as_deref(), Some("525007"));
        assert_eq!(feature.shape, Shape::Points);
        assert_eq!(feature.parts[0][0].z, Some(58.0));
        assert_eq!(feature.properties["@featureType"], "GeologicUnit");
    }

    #[test]
    fn projected_metres_are_refused_and_the_numbers_are_in_the_message() {
        // The one guard this module exists for. Lambert-93 is well-formed JSON.
        let text = collection(
            r#"{"type":"Feature","id":"x",
                "geometry":{"type":"Point","coordinates":[650000,6860000]},
                "properties":{}}"#,
        );
        let read = read(&text).expect("the document still reads");
        assert!(read.features.is_empty());
        assert!(read.refused[0].contains("650000"), "{}", read.refused[0]);
        assert!(read.refused[0].contains("WGS 84"));
    }

    #[test]
    fn a_declared_crs_that_is_not_wgs84_stops_the_whole_document() {
        // Different from the position guard, and it has to be: here the producer
        // *says* the file is projected. Reading it as degrees would be reading a
        // file against its own statement, and the numbers might well be in range.
        let text = r#"{"type":"FeatureCollection",
            "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::2154"}},
            "features":[]}"#;
        let why = read(text).expect_err("refused");
        assert!(why.0.contains("2154"), "{why}");
        // And the ordinary one is accepted, or every BRGM export would be refused:
        // they all carry `urn:ogc:def:crs:EPSG::4326`.
        let fine = r#"{"type":"FeatureCollection",
            "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::4326"}},
            "features":[]}"#;
        assert!(read(fine).is_ok());
    }

    #[test]
    fn one_bad_feature_does_not_take_the_others_with_it() {
        let text = collection(
            r#"{"type":"Feature","id":"good","geometry":{"type":"Point","coordinates":[2.29,48.87]},"properties":{}},
               {"type":"Feature","id":"bad","geometry":null,"properties":{}}"#,
        );
        let read = read(&text).expect("a collection");
        assert_eq!(read.features.len(), 1);
        assert_eq!(read.refused.len(), 1);
        assert!(read.refused[0].contains("bad"), "{}", read.refused[0]);
    }

    #[test]
    fn every_geometry_the_format_has_is_grouped_into_parts() {
        let text = collection(
            r#"{"type":"Feature","geometry":{"type":"MultiPoint","coordinates":[[1.0,2.0],[3.0,4.0]]},"properties":{}},
               {"type":"Feature","geometry":{"type":"LineString","coordinates":[[1.0,2.0],[3.0,4.0]]},"properties":{}},
               {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]},"properties":{}}"#,
        );
        let read = read(&text).expect("a collection");
        assert!(read.refused.is_empty(), "{:?}", read.refused);
        // Two places, two parts — never one part of two, which a caller could
        // join into a line that the file does not state.
        assert_eq!(read.features[0].parts.len(), 2);
        assert_eq!(read.features[1].parts.len(), 1);
        assert_eq!(read.features[1].parts[0].len(), 2);
        assert_eq!(read.features[2].shape, Shape::Areas);
    }

    #[test]
    fn a_ring_too_short_to_close_is_refused_rather_than_drawn_as_a_sliver() {
        let text = collection(
            r#"{"type":"Feature","id":"sliver",
                "geometry":{"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,1.0]]]},"properties":{}}"#,
        );
        let read = read(&text).expect("a collection");
        assert!(read.refused[0].contains("4 positions"), "{}", read.refused[0]);
    }

    #[test]
    fn a_geometry_collection_is_refused_by_name() {
        let text = collection(
            r#"{"type":"Feature","id":"mixed",
                "geometry":{"type":"GeometryCollection","geometries":[]},"properties":{}}"#,
        );
        let read = read(&text).expect("a collection");
        assert!(read.refused[0].contains("GeometryCollection"));
    }

    #[test]
    fn a_document_that_is_not_a_collection_fails_as_a_whole() {
        assert!(read("{").is_err());
        assert!(read(r#"{"type":"Feature"}"#).is_err());
        assert!(read(r#"{"type":"FeatureCollection"}"#).is_err());
    }
}

// -- Reading without holding the document -------------------------------------

/// What a caller keeps of each feature's `properties`.
///
/// # Why this exists beside the streaming reader, and matters more
///
/// Measured on a real harvest of 2 000 boreholes with their intervals
/// (50,8 MB of GeoJSON): reading it costs **1 185 MB** of peak memory, ×23 —
/// and the same 2 000 features with their properties removed weigh **3 MB**.
/// What costs is not reading the document, it is **keeping what nobody asked
/// for**: a map preview wants footprints, not `observationMethod`.
///
/// So the projection is a first-class argument rather than something a caller
/// does afterwards, because afterwards is too late: the memory has been spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Keep<'a> {
    /// Everything the producer wrote, nesting included. What [`read`] does, and
    /// what an operation that reads a vocabulary needs.
    #[default]
    All,
    /// None at all. The shape and the `id` still come back.
    None,
    /// Only these **top-level** keys of `properties`.
    ///
    /// Top-level and not dotted paths: a path language is a second query
    /// syntax, and the one thing a caller reliably knows is the name a producer
    /// puts at the top. A key that is not there is not an error — a collection
    /// where half the features carry a field is ordinary.
    Only(&'a [&'a str]),
}

/// What a caller wants out of a collection.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wanted<'a> {
    /// What to keep of each feature's properties.
    pub properties: Keep<'a>,
}

/// Whether the reader goes on.
///
/// **The only way to cancel.** Without it, stopping means reading to the end
/// and throwing the result away, which is the opposite of what a *Cancel*
/// button is for — and on a 50 MB answer it is nine seconds of nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Hand me the next one.
    Continue,
    /// I have what I came for.
    Stop,
}

/// What a streaming read did.
///
/// **It arrives at the end, and it has to.** Measured on the BRGM WFS: in its
/// GeoJSON, `"features"` starts at byte 28 and `numberMatched`,
/// `numberReturned` and `crs` are at byte 783 596 — the very end. A reader that
/// hands features over as they arrive cannot announce a total first, which is
/// why a count is asked for by a **separate** request (`resultType=hits`)
/// rather than read off the answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// How many features were handed to the sink.
    pub read: usize,
    /// One sentence per feature that could not be read, naming it.
    pub refused: Vec<String>,
    /// Whether the sink asked to stop before the end.
    pub stopped: bool,
    /// What the document said it matched, when it says so at all — a WFS
    /// answer carries it, a file written by hand does not.
    pub matched: Option<u64>,
    /// What the document said it returned, same caveat.
    pub returned: Option<u64>,
}

/// Read a `FeatureCollection` **from a stream**, one feature at a time.
///
/// Nothing of the document is held: not the text, not a `serde_json::Value` of
/// the whole of it. What is held at any moment is one feature plus whatever the
/// sink keeps — so the peak belongs to the caller, which is the point.
///
/// [`read`] is the same work with `Keep::All` and a sink that pushes into a
/// `Vec`; there is one implementation, and two faces of it.
///
/// # The legacy `crs` member, and the one thing streaming changes
///
/// [`read`] refuses a document declaring anything but WGS 84 — a Lambert-93
/// "GeoJSON" read as degrees puts a Paris-basin model in the Gulf of Guinea.
/// A stream can meet that member **after** the features, and then the refusal
/// comes too late to be a refusal. Both cases are handled and they are not the
/// same news:
///
/// - **before the features** — where a producer of such a file writes it, the
///   member being the 2008 form — nothing has been handed over and this
///   refuses, exactly like [`read`];
/// - **after** them, this still fails, and the message says the features were
///   already handed over, because a caller that drew them has to know to
///   throw them away.
pub fn read_streaming<R, S>(reader: R, wanted: &Wanted<'_>, sink: S) -> Result<Summary, Rejected>
where
    R: std::io::Read,
    S: FnMut(Feature) -> Flow,
{
    use serde::de::DeserializeSeed;

    // Buffered, because `serde_json`'s reader path asks for bytes in small
    // pieces and an unbuffered reader turns that into one syscall each.
    let reader = std::io::BufReader::with_capacity(64 * 1024, reader);
    let mut de = serde_json::Deserializer::from_reader(reader);
    Document { wanted, sink }
        .deserialize(&mut de)
        .map_err(|why| match why {
            // A refusal this module raised travels as a serde error and comes
            // back out as itself: wrapping it in "this is not JSON" would name
            // the wrong culprit.
            e if e.classify() == serde_json::error::Category::Data => Rejected(e.to_string()),
            e => Rejected(format!("this is not JSON: {e}")),
        })
}

/// The top-level object, visited key by key.
struct Document<'a, S> {
    wanted: &'a Wanted<'a>,
    sink: S,
}

impl<'de, 'a, S: FnMut(Feature) -> Flow> serde::de::DeserializeSeed<'de> for Document<'a, S> {
    type Value = Summary;

    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<Summary, D::Error> {
        de.deserialize_map(self)
    }
}

impl<'de, 'a, S: FnMut(Feature) -> Flow> serde::de::Visitor<'de> for Document<'a, S> {
    type Value = Summary;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a GeoJSON FeatureCollection")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(mut self, mut map: M) -> Result<Summary, M::Error> {
        use serde::de::Error;

        let mut out = Summary::default();
        let mut kind: Option<String> = None;
        let mut seen_features = false;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "type" => kind = Some(map.next_value()?),
                "crs" => {
                    // The legacy member. Read as a whole — it is three fields —
                    // and judged at once.
                    let value: Value = map.next_value()?;
                    if let Some(name) = value.get("properties").and_then(|p| p.get("name")).and_then(Value::as_str) {
                        let plain = name.rsplit(':').next().unwrap_or(name);
                        if !matches!(plain, "CRS84" | "4326" | "84") {
                            return Err(M::Error::custom(format!(
                                "the collection declares `{name}` — a GeoJSON is WGS 84 by \
                                 definition (RFC 7946), and this reader will not guess a \
                                 transformation; reproject the file first{}",
                                match seen_features {
                                    true => " — and it says so **after** its features, which \
                                             were already handed over: throw them away",
                                    false => "",
                                }
                            )));
                        }
                    }
                }
                "features" => {
                    // `type` may legally come after `features`, so it is checked
                    // once at the end rather than demanded here.
                    seen_features = true;
                    let seq = Features { wanted: self.wanted, sink: &mut self.sink, out: &mut out };
                    map.next_value_seed(seq)?;
                }
                "numberMatched" => out.matched = map.next_value::<Value>()?.as_u64(),
                "numberReturned" => out.returned = map.next_value::<Value>()?.as_u64(),
                // Everything else is skipped **without being built**: this is
                // where `totalFeatures`, `timeStamp`, `links` and whatever else
                // a producer adds would otherwise be allocated for nothing.
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        match kind.as_deref() {
            Some("FeatureCollection") => {}
            Some(other) => {
                return Err(M::Error::custom(format!(
                    "the document is a `{other}`, not a GeoJSON FeatureCollection"
                )));
            }
            None => {
                return Err(M::Error::custom(
                    "the document states no `type`, so it is not a GeoJSON FeatureCollection",
                ));
            }
        }
        if !seen_features {
            return Err(M::Error::custom("the collection has no `features` array"));
        }
        Ok(out)
    }
}

/// The `features` array, element by element.
struct Features<'a, S> {
    wanted: &'a Wanted<'a>,
    sink: &'a mut S,
    out: &'a mut Summary,
}

impl<'de, 'a, S: FnMut(Feature) -> Flow> serde::de::DeserializeSeed<'de> for Features<'a, S> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<(), D::Error> {
        de.deserialize_seq(self)
    }
}

impl<'de, 'a, S: FnMut(Feature) -> Flow> serde::de::Visitor<'de> for Features<'a, S> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an array of GeoJSON features")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut at = 0usize;
        // **One element at a time, and it dies at the end of the turn.** This
        // single line is what the whole seed exists for: `next_element` parses
        // exactly one feature out of the stream, and nothing accumulates but
        // what the sink chooses to keep.
        while let Some(entry) = seq.next_element::<Value>()? {
            match feature(&entry, at) {
                Ok(mut one) => {
                    project(&mut one, self.wanted.properties);
                    self.out.read += 1;
                    if (self.sink)(one) == Flow::Stop {
                        self.out.stopped = true;
                        // Drained rather than abandoned: leaving a half-read
                        // array behind makes the enclosing map read the rest of
                        // this feature as its own keys. It costs the parse of
                        // what is left and keeps none of it — and a caller who
                        // wanted the download to stop stops the *reader*, which
                        // is a door up (`Transport`).
                        while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                        return Ok(());
                    }
                }
                Err(why) => self.out.refused.push(why),
            }
            at += 1;
        }
        Ok(())
    }
}

/// Drop what the caller did not ask for, as early as anything here can.
fn project(feature: &mut Feature, keep: Keep<'_>) {
    match keep {
        Keep::All => {}
        Keep::None => feature.properties = Value::Null,
        Keep::Only(names) => {
            let Some(object) = feature.properties.as_object() else { return };
            let kept: serde_json::Map<String, Value> = names
                .iter()
                .filter_map(|name| object.get(*name).map(|v| ((*name).to_string(), v.clone())))
                .collect();
            feature.properties = Value::Object(kept);
        }
    }
}
