//! GeoJSON - features in WGS 84, carrying their identity.
//!
//! # Why this one refuses more than it writes
//!
//! RFC 7946 is explicit: coordinates are longitude and latitude in decimal
//! degrees, WGS 84. Not eastings. Not metres. The format has no CRS field any
//! more - the 2016 revision removed it precisely so that every GeoJSON means
//! the same thing.
//!
//! So writing a RESQML surface as GeoJSON is a **datum transformation**, not a
//! formatting job, and this crate does not carry a geodesy library (see
//! [`energyml_domain::reproject`] for why). Without a backend, this exporter
//! fails and says what is missing.
//!
//! That refusal is the whole point. Writing projected eastings into the
//! longitude field produces a file that every tool loads without complaint and
//! draws in the Gulf of Guinea, a few hundred kilometres off the coast of
//! Ghana - the place where (0, 0) is. It is the most recognisable wrong answer
//! in the business, and it comes from exactly this shortcut.
//!
//! # The escape hatch, explicit
//!
//! What the rule forbids is *silent* wrongness, not native coordinates. With
//! [`Options::native_crs`] the exporter writes the placed coordinates as they
//! are, accepts objects that name no CRS at all, and - when the EPSG code is
//! known and shared - embeds the **legacy 2008 `crs` member** that RFC 7946
//! removed and that QGIS and GDAL still honour, so the file says what it is.
//! The caller asked, the file is labelled: nothing is silent, and a package
//! under repair can be looked at before it can be reprojected.
//!
//! # What goes in a feature
//!
//! The identity, in `properties`: uuid, ETP URI, title, type, and the
//! interpretation and feature above it. A GeoJSON of anonymous polygons is a
//! picture; one that names what each shape is can be joined back to the model.

use std::io::Write;

use energyml_domain::reproject::Wgs84;
use energyml_geometry::{Placement, SceneObject, Topology};

use crate::error::{Error, Result};
use crate::registry::{Exporter, Options};

/// Writes RFC 7946 GeoJSON.
pub struct GeoJsonExporter;

impl Exporter for GeoJsonExporter {
    fn name(&self) -> &'static str {
        "geojson"
    }

    fn extension(&self) -> &'static str {
        "geojson"
    }

    fn mime(&self) -> &'static str {
        "application/geo+json"
    }

    fn describe(&self) -> &'static str {
        "RFC 7946 GeoJSON in WGS 84 - needs a reprojection backend"
    }

    fn placement(&self) -> Placement {
        Placement::Metres
    }

    fn handles(&self, object: &SceneObject) -> bool {
        // At least one **defined** node: an object whose every position is NaN
        // would pass the gate on its point count, be emptied by `prepare`, and
        // come out as a Feature with empty coordinates - a file every reader
        // loads and that shows nothing, seen from the interface.
        // Dropped and counted beats written empty.
        let has_defined = object.geometry.patches.iter().any(|patch| {
            patch
                .points
                .iter()
                .any(|point| point.iter().all(|v| v.is_finite()))
        });
        has_defined && object.epsg().is_some()
    }

    fn handles_with(&self, object: &SceneObject, options: &Options<'_>) -> bool {
        // A time axis still has to be placeable, in native mode as anywhere
        // else: the override widens which *coordinate systems* are accepted,
        // not which vertical quantities can be turned into a number.
        if !options.can_place_vertical(object) {
            return false;
        }
        if options.native_crs {
            // Native mode takes everything that has a shape: an object with no
            // CRS is precisely what the mode exists for.
            return !object.geometry.is_empty();
        }
        self.handles(object)
    }

    fn write(&self, object: &SceneObject, out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        write!(out, "{{\"type\":\"FeatureCollection\",")?;
        write_crs_member(std::slice::from_ref(object), out, options)?;
        writeln!(out, "\"features\":[")?;
        self.feature(object, out, options)?;
        writeln!(out, "\n]}}")?;
        Ok(())
    }

    fn write_all(&self, objects: &[SceneObject], out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        // One enclosing document, so the default "write them in order" is
        // wrong here - it would emit several collections end to end and produce
        // a file no parser accepts.
        write!(out, "{{\"type\":\"FeatureCollection\",")?;
        write_crs_member(objects, out, options)?;
        writeln!(out, "\"features\":[")?;
        let mut first = true;
        for object in objects {
            if !self.handles_with(object, options) {
                continue;
            }
            if !first {
                writeln!(out, ",")?;
            }
            first = false;
            self.feature(object, out, options)?;
        }
        writeln!(out, "\n]}}")?;
        Ok(())
    }
}

impl GeoJsonExporter {
    fn feature(&self, object: &SceneObject, out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        // Reshaped as the caller's rules ask, then stripped of the nodes that
        // have no position - one funnel, one order, for every format.
        // **`NaN` is not a JSON value**: one null Z among 34 000 positions was
        // enough to make a whole `.geojson` fail `JSON.parse`, and RFC 7946 is
        // JSON before it is anything else. See `registry::prepare`.
        let object = crate::registry::prepare(object, options)?;
        let object = object.as_ref();

        if options.native_crs {
            // Asked for explicitly: the placed coordinates, whatever system
            // they are in, no geodesy involved. The collection's `crs` member
            // (or its absence) is what says so to the reader.
            let placement = options.placement.unwrap_or(Placement::Projected);
            let coordinates = object.points_in(placement)?;
            return self.feature_body(object, &coordinates, out);
        }
        let Some(crs) = &object.crs else {
            return Err(Error::unsuitable(
                "geojson",
                format!("`{}`, which names no coordinate system", object.title),
            ));
        };
        if options.reprojector.is_none() && crs.projected.epsg() != Some(Wgs84::EPSG) {
            return Err(Error::Placement(format!(
                "GeoJSON is WGS 84 by definition (RFC 7946) and `{}` is in {}; \
                 supply a `Reprojector` - `energyml_proj::Proj4` is one - because \
                 writing projected metres into a longitude field puts the model in \
                 the Gulf of Guinea",
                object.title,
                match crs.projected.epsg() {
                    Some(code) => format!("EPSG:{code}"),
                    None => format!("{:?}", crs.projected),
                }
            )));
        }

        // Deliberately not `options.placement`: RFC 7946 fixes the answer at
        // WGS 84, so the only open question is which backend gets there.
        //
        // `points_in_epsg` places to metres **once** and then reprojects. The
        // shape this replaced placed the points and then called `to_epsg`,
        // which places them again - offsets, rotation, unit scale and the depth
        // flip all applied twice. On a CRS whose origin is (0, 0) with no
        // rotation, which is every package in the corpus, that is exactly zero
        // difference; on a real one it is not.
        let coordinates = options.points_in_epsg(object, Wgs84::EPSG)?;
        self.feature_body(object, &coordinates, out)
    }

    /// One feature, from coordinates already in the system the document says.
    fn feature_body(
        &self,
        object: &SceneObject,
        coordinates: &[[f64; 3]],
        out: &mut dyn Write,
    ) -> Result<()> {
        // Geometry: the topology decides which GeoJSON type this is. A
        // triangulated set is a MultiPolygon of three-cornered rings; a
        // polyline set is a MultiLineString; anything else is a MultiPoint,
        // which is honest rather than clever.
        let geometry = match first_topology(object) {
            Some(Topology::Triangles(_)) => triangles(object, coordinates),
            Some(Topology::Quads(_)) | Some(Topology::Polygons(_)) => rings(object, coordinates),
            Some(Topology::Polylines(_) | Topology::Segments(_)) => polylines(object, coordinates),
            _ => multipoint(coordinates),
        };

        write!(
            out,
            "{{\"type\":\"Feature\",{}\"geometry\":{geometry},\"properties\":{{",
            bbox(coordinates)
        )?;
        let mut properties: Vec<String> = vec![
            pair("uuid", &object.uuid),
            pair("uri", &object.uri),
            pair("title", &object.title),
            pair("type", &object.qualified_type),
            pair("crs", &object.uuid_of_crs()),
        ];
        if let Some(interpretation) = &object.interpretation {
            properties.push(pair("interpretation_uuid", &interpretation.uuid));
            properties.push(pair("interpretation_title", &interpretation.title));
        }
        if let Some(feature) = &object.feature {
            properties.push(pair("feature_uuid", &feature.uuid));
            properties.push(pair("feature_title", &feature.title));
        }
        // What the caller's `--metadata` spec asked for. Written here and not
        // only in VTK: the module documentation of `SceneObject::metadata`
        // promises "the same spec produces the same keys in an OBJ comment, a
        // GeoJSON property and a VTK field array", and until it was fixed only the
        // third was true - a spec passed to a GeoJSON export produced nothing
        // at all, silently.
        //
        // A key that found nothing is written `null` rather than left out. A
        // FeatureCollection is read as a table, and a column that disappears on
        // the rows where it is empty is a different table.
        for (key, value) in object.metadata.entries() {
            let key = sanitise_key(key);
            if properties.iter().any(|p| p.starts_with(&format!("\"{key}\":"))) {
                continue;
            }
            match value {
                Some(value) => properties.push(pair(&key, value)),
                None => properties.push(format!("\"{key}\":null")),
            }
        }
        // A property has one value per node or per cell, and a GeoJSON feature
        // has one set of properties - so what goes in is the **summary**, and
        // it is labelled as such. Writing the first value, or the mean under
        // the property's own name, would look like the value of the object.
        for attribute in &object.attributes {
            let name = sanitise_key(&attribute.name);
            let finite: Vec<f64> = attribute
                .values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            properties.push(format!("\"{name}_count\":{}", attribute.values.len()));
            if let (Some(min), Some(max)) = (
                finite.iter().copied().reduce(f64::min),
                finite.iter().copied().reduce(f64::max),
            ) {
                let mean = finite.iter().sum::<f64>() / finite.len() as f64;
                properties.push(format!("\"{name}_min\":{min}"));
                properties.push(format!("\"{name}_max\":{max}"));
                properties.push(format!("\"{name}_mean\":{mean}"));
            }
            if let Some(uom) = &attribute.uom {
                properties.push(pair(&format!("{name}_uom"), uom));
            }
            // The one case where a single number *is* the value of the object.
            if attribute.values.len() == 1 && attribute.values[0].is_finite() {
                properties.push(format!("\"{name}\":{}", attribute.values[0]));
            }
        }
        write!(out, "{}", properties.join(","))?;
        write!(out, "}}}}")?;
        Ok(())
    }
}

/// A property name usable as a JSON key: no quotes, no control characters.
fn sanitise_key(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// The legacy 2008 `crs` member, in native mode, when there is one to state.
///
/// RFC 7946 removed it; QGIS and GDAL still honour it, and a knowingly
/// non-conformant file that names its system beats one that says nothing.
///
/// Written when every object that **names** a code names the same one. The
/// objects that name none ride along under that label - deliberately: they
/// were written by the same producer at the same site, and showing them next
/// to the georeferenced ones is exactly how a person checks whether their
/// missing CRS was the labelled one. Two objects naming *different* codes get
/// no member rather than a wrong one.
fn write_crs_member(objects: &[SceneObject], out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
    if !options.native_crs {
        return Ok(());
    }
    let mut stated = objects
        .iter()
        .filter(|o| !o.geometry.is_empty())
        .filter_map(|o| o.epsg());
    let Some(code) = stated.next() else {
        return Ok(());
    };
    if stated.all(|c| c == code) {
        write!(
            out,
            "\"crs\":{{\"type\":\"name\",\"properties\":{{\"name\":\"urn:ogc:def:crs:EPSG::{code}\"}}}},"
        )?;
    }
    Ok(())
}

trait CrsUuid {
    fn uuid_of_crs(&self) -> String;
}

impl CrsUuid for SceneObject {
    fn uuid_of_crs(&self) -> String {
        self.crs.as_ref().map(|c| c.uuid.clone()).unwrap_or_default()
    }
}

fn first_topology(object: &SceneObject) -> Option<&Topology> {
    object.geometry.patches.first().map(|patch| &patch.topology)
}

fn triangles(object: &SceneObject, points: &[[f64; 3]]) -> String {
    let (_, indices) = object.geometry.merged();
    let rings: Vec<String> = indices
        .iter()
        .filter(|t| t.iter().all(|i| (*i as usize) < points.len()))
        .map(|t| {
            // A GeoJSON ring is closed: the last position repeats the first.
            let ring: Vec<String> = [t[0], t[1], t[2], t[0]]
                .iter()
                .map(|i| position(points[*i as usize]))
                .collect();
            format!("[[{}]]", ring.join(","))
        })
        .collect();
    format!(
        "{{\"type\":\"MultiPolygon\",\"coordinates\":[{}]}}",
        rings.join(",")
    )
}

/// Rings, as a `Polygon` when there is one and a `MultiPolygon` when there are
/// several.
///
/// What a 2D grid's cells are, and what someone opening a horizon in QGIS
/// wants: polygons they can colour and query, not three hundred thousand dots.
/// The ring closes on its first position, as RFC 7946 §3.1.6 requires, and its
/// winding was already measured in `energyml_geometry::shape` - the RFC asks an
/// exterior ring to be counterclockwise, and no RESQML file says which way its
/// grid runs.
///
/// **Singular when singular**, and that is not cosmetic: an outline is *one*
/// boundary, and OSDU's `SpatialGeometryTypeID` distinguishes a `Polygon` from a
/// `MultiPolygon`. Wrapping a lone ring would name the wrong type in a field
/// whose whole job is to say which one it is.
fn rings(object: &SceneObject, points: &[[f64; 3]]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut base = 0usize;
    for patch in &object.geometry.patches {
        let cells: Vec<Vec<usize>> = match &patch.topology {
            Topology::Quads(quads) => quads
                .iter()
                .map(|quad| quad.iter().map(|c| base + *c as usize).collect())
                .collect(),
            Topology::Polygons(rings) => rings
                .iter()
                .map(|ring| ring.iter().map(|c| base + *c as usize).collect())
                .collect(),
            _ => Vec::new(),
        };
        for corners in cells {
            if corners.len() < 3 || corners.iter().any(|c| *c >= points.len()) {
                continue;
            }
            let ring: Vec<String> = corners
                .iter()
                .chain(std::iter::once(&corners[0]))
                .map(|i| position(points[*i]))
                .collect();
            out.push(format!("[{}]", ring.join(",")));
        }
        base += patch.points.len();
    }
    match out.len() {
        // A `Polygon`'s coordinates are an **array of rings** - the exterior
        // one, then any holes - not the ring itself. Writing the ring directly
        // produces a document that parses and means something else entirely.
        1 => format!("{{\"type\":\"Polygon\",\"coordinates\":[{}]}}", out[0]),
        _ => format!(
            "{{\"type\":\"MultiPolygon\",\"coordinates\":[{}]}}",
            out.iter().map(|r| format!("[{r}]")).collect::<Vec<_>>().join(",")
        ),
    }
}

fn polylines(object: &SceneObject, points: &[[f64; 3]]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut start = 0usize;
    for patch in &object.geometry.patches {
        // A wireframe's edges: each pair is a two-position line. Written
        // here rather than in a writer of its own so a mixed object keeps one
        // MultiLineString.
        if let Topology::Segments(pairs) = &patch.topology {
            for pair in pairs {
                let (a, b) = (start + pair[0] as usize, start + pair[1] as usize);
                if a < points.len() && b < points.len() {
                    lines.push(format!("[{},{}]", position(points[a]), position(points[b])));
                }
            }
        }
        if let Topology::Polylines(runs) = &patch.topology {
            let mut at = start;
            for (rank, run) in runs.iter().enumerate() {
                let end = (at + *run as usize).min(points.len());
                if end > at + 1 {
                    let mut line: Vec<String> = points[at..end].iter().map(|p| position(*p)).collect();
                    // A ring the file calls closed repeats its first position,
                    // the only way a LineString says so (RFC 7946 §3.1.4 has
                    // no flag). Only when stated: an absent `ClosedPolylines`
                    // is not a ring.
                    if patch.closed.get(rank) == Some(&true) {
                        line.push(position(points[at]));
                    }
                    lines.push(format!("[{}]", line.join(",")));
                }
                at = end;
            }
        }
        start += patch.points.len();
    }
    format!(
        "{{\"type\":\"MultiLineString\",\"coordinates\":[{}]}}",
        lines.join(",")
    )
}

/// Points, as a `Point` when there is one and a `MultiPoint` when there are
/// several.
///
/// Singular when singular, for the same reason as [`rings`]: a centroid is
/// *one* point, and OSDU's `SpatialPoint` asks for a `Point`.
fn multipoint(points: &[[f64; 3]]) -> String {
    if let [only] = points {
        return format!("{{\"type\":\"Point\",\"coordinates\":{}}}", position(*only));
    }
    let positions: Vec<String> = points.iter().map(|p| position(*p)).collect();
    format!(
        "{{\"type\":\"MultiPoint\",\"coordinates\":[{}]}}",
        positions.join(",")
    )
}

/// The bounding box of a set of positions, as RFC 7946 §5 writes it.
///
/// `[west, south, east, north]` in 2D, `[west, south, low, east, north, high]`
/// in 3D - **the axis order interleaves**, which is the trap: it is not the two
/// corners one after the other. Written with a trailing comma so it can be
/// dropped into an object, and empty when there is nothing to bound.
///
/// Optional in the RFC and optional in OSDU's schema. Written anyway, because
/// it is what lets a viewer or an index decide whether to open a ten-megabyte
/// file at all, and it costs one pass over numbers that are already in hand.
fn bbox(points: &[[f64; 3]]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    if low.iter().chain(high.iter()).any(|v| !v.is_finite()) {
        return String::new();
    }
    format!(
        "\"bbox\":[{:.9},{:.9},{:.3},{:.9},{:.9},{:.3}],",
        low[0], low[1], low[2], high[0], high[1], high[2]
    )
}

/// One position: longitude, latitude, elevation.
///
/// RFC 7946 §3.1.1 fixes that order, and it is the reverse of how most people
/// say it. Nine decimals is about a tenth of a millimetre, past any survey.
fn position(point: [f64; 3]) -> String {
    format!("[{:.9},{:.9},{:.3}]", point[0], point[1], point[2])
}

fn pair(key: &str, value: &str) -> String {
    format!("\"{key}\":\"{}\"", escape(value))
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
