//! Terrain and imagery sources - a catalogue, one request builder, one reader.
//!
//! The need: show digital terrain model (DTM) data in the 3D canvas, as textured
//! areas with relief, from public data and APIs whose access rights have been
//! checked; and manage catalogues for the inputs of the terrain operation, so
//! that other services, abroad for instance, can be added later.
//!
//! # A protocol is code, a source is data
//!
//! What a WMS raster *is* - how to ask it for a window, how to read the answer -
//! is written once, here. Which services exist, over which area, at which
//! resolution, under which licence, is a **catalogue**: a JSON file
//! (`catalogue/terrain.json`, embedded) that a user can extend or override with
//! one of their own ([`TerrainCatalogue::with_user`]) without a line of Rust.
//! A service abroad is an entry, not a release.
//!
//! # Purity
//!
//! Nothing here does I/O. A request is *built* ([`wms_raster_request`]), the
//! round trip is the existing [`crate::Transport`], and the answer is *read*
//! ([`read_geotiff`], [`lattice_of`]) from bytes. All of it is tested without a
//! network.
//!
//! # The axis trap, again
//!
//! WMS 1.3.0 gives the BBOX of a geographic CRS in the CRS's own axis order:
//! EPSG:4326 is **latitude, longitude**. A projected CRS such as EPSG:2154 is
//! easting, northing. The caller of [`wms_raster_request`] always passes the
//! window as `[x_min, y_min, x_max, y_max]` - longitude/easting first - and the
//! builder swaps for the CRS that wants it, so the decision is made in one
//! place and pinned by a test, never left to each caller.
//!
//! # Rows go north to south in a GeoTIFF, south to north in a lattice
//!
//! The house order (`energyml_build::pieces::Lattice`) is `z[j*ni + i]` with `j`
//! along **+y**, origin at the **south-west pixel centre**. A GeoTIFF stores its
//! first row at the **north** edge and tiepoints the top-left **corner** of the
//! top-left pixel (PixelIsArea). [`lattice_of`] shifts by half a pixel and
//! flips the rows; forgetting either gives a terrain that is mirrored or
//! shifted and looks entirely plausible.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::transport::Request;
use crate::wfs::encode;

/// What a source provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// Heights: a terrain or surface model.
    Relief,
    /// Pictures to drape: orthophotos, satellite mosaics.
    Imagery,
    /// Building footprints and heights.
    Buildings,
}

impl SourceKind {
    /// The word used in messages.
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Relief => "relief",
            SourceKind::Imagery => "imagery",
            SourceKind::Buildings => "buildings",
        }
    }
}

/// How a source is spoken to. **Code**: each variant has a reader (or is
/// refused by name where it has none yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Protocol {
    /// A WMS serving numeric rasters (`GetMap`, WMS 1.3.0).
    WmsRaster {
        /// The service address, without parameters.
        url: String,
        /// The layer name.
        layer: String,
        /// The MIME type to ask for, e.g. `image/geotiff`.
        format: String,
    },
    /// A WMTS tile service.
    #[serde(rename_all = "camelCase")]
    Wmts {
        /// The service address, without parameters.
        url: String,
        /// The layer name.
        layer: String,
        /// The tile matrix set, e.g. `PM` for Web Mercator.
        tile_matrix_set: String,
        /// The MIME type of a tile.
        format: String,
        /// The style asked for - WMTS requires one. The IGN Géoplateforme
        /// answers `normal`; left out, the standard's `default`.
        #[serde(default = "default_style")]
        style: String,
    },
    /// Elevation encoded in the colours of a PNG tile (the *terrarium* scheme).
    XyzTerrarium {
        /// An address with `{z}`, `{x}` and `{y}` placeholders.
        template: String,
    },
    /// Cloud Optimized GeoTIFFs in a bucket.
    Cog {
        /// The bucket address.
        url: String,
    },
}

/// The WMTS style a source that names none is asked in (OGC 07-057r7, §7.2.4.6).
fn default_style() -> String {
    "default".to_string()
}

/// Where a source has data: WGS 84 boxes, `[west, south, east, north]` in
/// degrees. Several boxes allowed: a source may cover two disjoint regions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    /// The boxes, `[west, south, east, north]`.
    pub boxes: Vec<[f64; 4]>,
}

impl Coverage {
    /// Whether one box **contains** the whole of `area`. A source that covers
    /// half of an area is not offered for it: a half terrain is a wrong one.
    pub fn contains(&self, area: [f64; 4]) -> bool {
        self.boxes
            .iter()
            .any(|b| b[0] <= area[0] && b[1] <= area[1] && b[2] >= area[2] && b[3] >= area[3])
    }
}

/// The vertical reference of a relief source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vertical {
    /// The unit, e.g. `m`.
    pub unit: String,
    /// The datum, as text: heights are not comparable across datums.
    pub datum: String,
}

/// One entry of the catalogue. **Data**.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainSource {
    /// The short name that identifies it, and that a user entry replaces by.
    pub id: String,
    /// A line for a list.
    pub title: String,
    /// Relief, imagery or buildings.
    pub kind: SourceKind,
    /// How it is spoken to.
    pub protocol: Protocol,
    /// Where it has data.
    pub coverage: Coverage,
    /// The EPSG codes it can render in.
    pub crs: Vec<u32>,
    /// The ground size of a pixel, in metres. Smaller is better.
    pub resolution_m: f64,
    /// Vertical unit and datum, for relief.
    #[serde(default)]
    pub vertical: Option<Vertical>,
    /// The licence, as the publisher words it.
    pub licence: String,
    /// What must be shown to the reader.
    pub attribution: String,
    /// Whether a key is needed. Nothing here sends one.
    pub needs_key: bool,
    /// Caveats, and what was not verified.
    #[serde(default)]
    pub note: Option<String>,
}

/// The file shape, and the in-memory shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainCatalogue {
    sources: Vec<TerrainSource>,
}

impl TerrainCatalogue {
    /// The catalogue shipped with the crate.
    pub fn builtin() -> Result<Self> {
        Self::parse(include_str!("../catalogue/terrain.json"))
    }

    /// Read a catalogue from JSON: `{"sources": [ … ]}`.
    ///
    /// Refuses, by name, an unreadable document, a duplicated `id`, an entry
    /// with an empty `id`, and a coverage that is empty or has an impossible
    /// box - a catalogue that parses must be one `covering` can trust.
    pub fn parse(json: &str) -> Result<Self> {
        let catalogue: TerrainCatalogue = serde_json::from_str(json)
            .map_err(|why| Error::Unreadable(format!("terrain catalogue: {why}")))?;
        let mut seen: Vec<&str> = Vec::new();
        for source in &catalogue.sources {
            if source.id.trim().is_empty() {
                return Err(Error::Unreadable(
                    "terrain catalogue: a source has an empty id".into(),
                ));
            }
            if seen.contains(&source.id.as_str()) {
                return Err(Error::Unreadable(format!(
                    "terrain catalogue: the id `{}` appears twice",
                    source.id
                )));
            }
            seen.push(&source.id);
            if source.coverage.boxes.is_empty() {
                return Err(Error::Unreadable(format!(
                    "terrain catalogue: source `{}` covers nothing (no box)",
                    source.id
                )));
            }
            for b in &source.coverage.boxes {
                let sane = b.iter().all(|v| v.is_finite())
                    && b[0] < b[2]
                    && b[1] < b[3]
                    && b[1] >= -90.0
                    && b[3] <= 90.0;
                if !sane {
                    return Err(Error::Unreadable(format!(
                        "terrain catalogue: source `{}` has an impossible coverage box {b:?} \
                         (expected [west, south, east, north] in degrees)",
                        source.id
                    )));
                }
            }
        }
        Ok(catalogue)
    }

    /// Merge a user catalogue: its entries come **first** (so they are tried
    /// first, and win a tie on resolution), and a user entry with the same
    /// `id` replaces the built-in one rather than sitting beside it.
    pub fn with_user(self, user: TerrainCatalogue) -> Self {
        let mut sources = user.sources;
        for source in self.sources {
            if !sources.iter().any(|mine| mine.id == source.id) {
                sources.push(source);
            }
        }
        TerrainCatalogue { sources }
    }

    /// Every entry, in trial order.
    pub fn sources(&self) -> &[TerrainSource] {
        &self.sources
    }

    /// One entry by its `id`.
    pub fn get(&self, id: &str) -> Option<&TerrainSource> {
        self.sources.iter().find(|source| source.id == id)
    }

    /// The sources of `kind` whose coverage contains `area_wgs84`
    /// (`[west, south, east, north]`), best resolution (smallest metres) first;
    /// equal resolutions keep the catalogue order.
    ///
    /// **Empty means the caller must refuse**, by name: see
    /// [`TerrainCatalogue::refusal`].
    pub fn covering(&self, kind: SourceKind, area_wgs84: [f64; 4]) -> Vec<&TerrainSource> {
        let mut found: Vec<&TerrainSource> = self
            .sources
            .iter()
            .filter(|source| source.kind == kind && source.coverage.contains(area_wgs84))
            .collect();
        // `sort_by` is stable: ties keep the catalogue order, user entries first.
        found.sort_by(|a, b| a.resolution_m.total_cmp(&b.resolution_m));
        found
    }

    /// The refusal for an area nothing covers: it names the area and the
    /// sources that were considered, so that the reader knows whether the
    /// catalogue is missing an entry or the area is out of reach.
    pub fn refusal(&self, kind: SourceKind, area_wgs84: [f64; 4]) -> Error {
        let considered: Vec<String> = self
            .sources
            .iter()
            .filter(|source| source.kind == kind)
            .map(|source| format!("`{}` ({:?})", source.id, source.coverage.boxes))
            .collect();
        let tried = if considered.is_empty() {
            format!("the catalogue has no {} source at all", kind.label())
        } else {
            format!("considered: {}", considered.join(", "))
        };
        Error::Refused(format!(
            "no {} source covers the area [west {}, south {}, east {}, north {}] (WGS 84 degrees); {tried}",
            kind.label(),
            area_wgs84[0],
            area_wgs84[1],
            area_wgs84[2],
            area_wgs84[3],
        ))
    }
}

/// The most pixels a WMS answer may have on either side, as measured on the IGN
/// service (`MaxWidth`/`MaxHeight` 5010). Refused here rather than
/// sent: a request known to be condemned is a round trip wasted.
pub const MAX_PIXELS: u32 = 5010;

/// Whether WMS 1.3.0 wants latitude before longitude in the BBOX of `epsg`.
///
/// **The axis order is the CRS's own** (WMS 1.3.0, §6.7.3.3). Only the
/// geographic codes this module can meet are listed; a projected code is
/// easting, northing. A code missing from this list would silently be sent as
/// x,y, which is why a source declares the codes it renders and the builder
/// refuses any other.
pub fn lat_lon_first(epsg: u32) -> bool {
    matches!(epsg, 4326 | 4258)
}

/// The `GetMap` for a WMS raster source.
///
/// `bbox` is `[x_min, y_min, x_max, y_max]` **in the CRS `epsg`, longitude or
/// easting first** - whatever the CRS wants on the wire, the builder swaps.
/// Refuses: a protocol that is not a WMS raster (named, *not read yet*); an
/// EPSG the source does not list; a degenerate or non-finite window; a width or
/// height of 0 or above [`MAX_PIXELS`].
pub fn wms_raster_request(
    source: &TerrainSource,
    bbox: [f64; 4],
    epsg: u32,
    width: u32,
    height: u32,
) -> Result<Request> {
    let (url, layer, format) = match &source.protocol {
        Protocol::WmsRaster { url, layer, format } => (url, layer, format),
        other => {
            let name = match other {
                Protocol::Wmts { .. } => "wmts",
                Protocol::XyzTerrarium { .. } => "xyzTerrarium",
                Protocol::Cog { .. } => "cog",
                Protocol::WmsRaster { .. } => "wmsRaster",
            };
            return Err(Error::Refused(format!(
                "source `{}` speaks `{name}`, which is not read yet; only wmsRaster has a request builder",
                source.id
            )));
        }
    };
    if !source.crs.contains(&epsg) {
        return Err(Error::Refused(format!(
            "source `{}` does not list EPSG:{epsg}; it renders {:?}",
            source.id, source.crs
        )));
    }
    if width == 0 || height == 0 || width > MAX_PIXELS || height > MAX_PIXELS {
        return Err(Error::Refused(format!(
            "a {width}x{height} window is refused: each side must be between 1 and {MAX_PIXELS} \
             (the service's MaxWidth/MaxHeight)"
        )));
    }
    if !bbox.iter().all(|v| v.is_finite()) || bbox[0] >= bbox[2] || bbox[1] >= bbox[3] {
        return Err(Error::Refused(format!(
            "the window {bbox:?} is not [x_min, y_min, x_max, y_max] with min < max"
        )));
    }
    let wire = if lat_lon_first(epsg) {
        [bbox[1], bbox[0], bbox[3], bbox[2]]
    } else {
        bbox
    };
    let list = format!("{},{},{},{}", wire[0], wire[1], wire[2], wire[3]);
    let url = format!(
        "{url}?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS={}&STYLES=&CRS={}&BBOX={}&WIDTH={width}&HEIGHT={height}&FORMAT={}",
        encode(layer),
        encode(&format!("EPSG:{epsg}")),
        encode(&list),
        encode(format),
    );
    Ok(Request {
        url,
        accept: format.clone(),
        about: format!(
            "{} window {width}x{height} of `{}` in EPSG:{epsg} (BBOX on the wire: {})",
            source.kind.label(),
            source.id,
            if lat_lon_first(epsg) { "lat,lon" } else { "x,y" }
        ),
    })
}

/// A decoded single-band raster, in the file's own order.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    /// Pixels along x.
    pub width: usize,
    /// Pixels along y.
    pub height: usize,
    /// `[x, y]` of the **top-left corner** of the top-left pixel.
    pub origin_top_left: [f64; 2],
    /// `[dx, dy]`, both positive: the pixel size, `dy` counted downwards.
    pub pixel: [f64; 2],
    /// Row-major, **first row = north**.
    pub values: Vec<f64>,
    /// The value that means *no data*, when the file says so.
    pub nodata: Option<f64>,
}

fn bad(why: impl Into<String>) -> Error {
    Error::Unreadable(format!("GeoTIFF: {}", why.into()))
}

fn truncated() -> Error {
    bad("the file is truncated")
}

fn slice(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    at.checked_add(len)
        .and_then(|end| bytes.get(at..end))
        .ok_or_else(truncated)
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let s = slice(bytes, at, 2)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let s = slice(bytes, at, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// One IFD entry: where its value lives.
struct Entry {
    ty: u16,
    count: usize,
    /// The position of the 4-byte value/offset field.
    field: usize,
}

impl Entry {
    fn data<'a>(&self, bytes: &'a [u8]) -> Result<&'a [u8]> {
        let size: usize = match self.ty {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 | 16 | 17 | 18 => 8,
            other => return Err(bad(format!("a tag has the unknown field type {other}"))),
        };
        let len = size.checked_mul(self.count).ok_or_else(truncated)?;
        if len <= 4 {
            slice(bytes, self.field, len)
        } else {
            let at = u32_at(bytes, self.field)? as usize;
            slice(bytes, at, len)
        }
    }
}

struct Ifd<'a> {
    bytes: &'a [u8],
    entries: HashMap<u16, Entry>,
}

impl Ifd<'_> {
    fn has(&self, tag: u16) -> bool {
        self.entries.contains_key(&tag)
    }

    /// The unsigned values of a SHORT or LONG tag.
    fn uints(&self, tag: u16) -> Result<Option<Vec<u64>>> {
        let Some(entry) = self.entries.get(&tag) else {
            return Ok(None);
        };
        let data = entry.data(self.bytes)?;
        match entry.ty {
            3 => Ok(Some(
                fixed_chunks::<_, 2>(data)
                    .map(|c| u16::from_le_bytes(*c) as u64)
                    .collect(),
            )),
            4 => Ok(Some(
                fixed_chunks::<_, 4>(data)
                    .map(|c| u32::from_le_bytes(*c) as u64)
                    .collect(),
            )),
            other => Err(bad(format!(
                "tag {tag} has field type {other}, expected SHORT or LONG"
            ))),
        }
    }

    /// The first value of a SHORT/LONG tag, or `default` when absent.
    fn scalar(&self, tag: u16, default: u64) -> Result<u64> {
        Ok(self
            .uints(tag)?
            .and_then(|v| v.first().copied())
            .unwrap_or(default))
    }

    fn doubles(&self, tag: u16) -> Result<Option<Vec<f64>>> {
        let Some(entry) = self.entries.get(&tag) else {
            return Ok(None);
        };
        if entry.ty != 12 {
            return Err(bad(format!(
                "tag {tag} has field type {}, expected DOUBLE",
                entry.ty
            )));
        }
        let data = entry.data(self.bytes)?;
        Ok(Some(
            fixed_chunks::<_, 8>(data)
                .map(|c| f64::from_le_bytes(*c))
                .collect(),
        ))
    }

    fn ascii(&self, tag: u16) -> Result<Option<String>> {
        let Some(entry) = self.entries.get(&tag) else {
            return Ok(None);
        };
        let data = entry.data(self.bytes)?;
        Ok(Some(
            String::from_utf8_lossy(data)
                .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_string(),
        ))
    }
}

/// Read a GeoTIFF: **baseline, little-endian, uncompressed, one band, float32,
/// strips** - the form the IGN service answers in (measured).
///
/// Reads `ModelPixelScale` (33550), `ModelTiepoint` (33922) and `GDAL_NODATA`
/// (42113). Refuses by name, never by guessing: big-endian and BigTIFF files,
/// any compression, tiles, more than one band, any sample format or depth but
/// float32, a missing or non-trivial georeferencing, and `PixelIsPoint`
/// rasters (their tiepoint is a centre, not a corner, and reading it as a
/// corner would shift the terrain by half a pixel).
pub fn read_geotiff(bytes: &[u8]) -> Result<Raster> {
    let head = slice(bytes, 0, 8)?;
    match (head[0], head[1]) {
        (b'I', b'I') => {}
        (b'M', b'M') => return Err(bad("big-endian files are not read (only little-endian)")),
        _ => return Err(bad("not a TIFF: the first two bytes are neither `II` nor `MM`")),
    }
    match u16_at(bytes, 2)? {
        42 => {}
        43 => return Err(bad("BigTIFF is not read")),
        other => {
            return Err(bad(format!(
                "not a TIFF: the magic number is {other}, expected 42"
            )));
        }
    }

    let ifd_at = u32_at(bytes, 4)? as usize;
    let n = u16_at(bytes, ifd_at)? as usize;
    let table = slice(bytes, ifd_at + 2, n * 12)?;
    let mut entries = HashMap::new();
    for (k, raw) in fixed_chunks::<_, 12>(table).enumerate() {
        let tag = u16::from_le_bytes([raw[0], raw[1]]);
        entries.insert(
            tag,
            Entry {
                ty: u16::from_le_bytes([raw[2], raw[3]]),
                count: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize,
                field: ifd_at + 2 + k * 12 + 8,
            },
        );
    }
    let ifd = Ifd { bytes, entries };

    let compression = ifd.scalar(259, 1)?;
    if compression != 1 {
        return Err(bad(format!(
            "compression {compression} is not read; only 1 (uncompressed)"
        )));
    }
    if ifd.has(322) || ifd.has(323) {
        return Err(bad("tiles are not read; only strips"));
    }
    let samples = ifd.scalar(277, 1)?;
    if samples != 1 {
        return Err(bad(format!("{samples} bands are not read; only one")));
    }
    let bits = ifd.scalar(258, 1)?;
    let format = ifd.scalar(339, 1)?;
    if bits != 32 || format != 3 {
        return Err(bad(format!(
            "{bits}-bit samples of SampleFormat {format} are not read; only float32 (32 bits, SampleFormat 3)"
        )));
    }

    let width = ifd.scalar(256, 0)? as usize;
    let height = ifd.scalar(257, 0)? as usize;
    if width == 0 || height == 0 {
        return Err(bad("the image has no width or no height"));
    }

    // PixelIsPoint would put the tiepoint on a pixel centre. GeoKey 1025
    // (GTRasterTypeGeoKey): 1 = PixelIsArea, 2 = PixelIsPoint.
    if let Some(keys) = ifd.uints(34735)? {
        for key in fixed_chunks::<_, 4>(keys.get(4..).unwrap_or(&[])) {
            if key[0] == 1025 && key[1] == 0 && key[3] == 2 {
                return Err(bad("PixelIsPoint rasters are not read; only PixelIsArea"));
            }
        }
    }

    let (Some(scale), Some(tie)) = (ifd.doubles(33550)?, ifd.doubles(33922)?) else {
        return Err(bad(
            "no georeferencing: ModelPixelScale (33550) and ModelTiepoint (33922) are both needed",
        ));
    };
    if scale.len() < 2 {
        return Err(bad("ModelPixelScale has fewer than two values"));
    }
    if tie.len() != 6 {
        return Err(bad(format!(
            "ModelTiepoint has {} values; only a single tiepoint (6 values) is read",
            tie.len()
        )));
    }
    if !(scale[0] > 0.0 && scale[1] > 0.0) {
        return Err(bad("ModelPixelScale is not positive"));
    }
    // The tiepoint ties raster (i, j) to model (x, y); (0, 0) is the usual
    // case, and any other is handled by moving back to the corner.
    let origin_top_left = [tie[3] - tie[0] * scale[0], tie[4] + tie[1] * scale[1]];

    let rows_per_strip = (ifd.scalar(278, height as u64)? as usize).clamp(1, height);
    let offsets = ifd.uints(273)?.ok_or_else(|| bad("no StripOffsets (273)"))?;
    let counts = ifd.uints(279)?.ok_or_else(|| bad("no StripByteCounts (279)"))?;
    let strips = height.div_ceil(rows_per_strip);
    if offsets.len() != strips || counts.len() != strips {
        return Err(bad(format!(
            "{} strip offset(s) and {} byte count(s) for {strips} strip(s)",
            offsets.len(),
            counts.len()
        )));
    }

    let mut values = Vec::with_capacity(width.checked_mul(height).ok_or_else(truncated)?);
    for (s, (&offset, &count)) in offsets.iter().zip(&counts).enumerate() {
        let rows = rows_per_strip.min(height - s * rows_per_strip);
        let need = rows * width * 4;
        if (count as usize) < need {
            return Err(bad(format!("strip {s} holds {count} bytes, {need} are needed")));
        }
        let data = slice(bytes, offset as usize, need)?;
        values.extend(fixed_chunks::<_, 4>(data).map(|c| f32::from_le_bytes(*c) as f64));
    }

    let nodata = match ifd.ascii(42113)? {
        None => None,
        Some(text) if text.is_empty() => None,
        Some(text) => Some(
            text.parse::<f64>()
                .map_err(|_| bad(format!("GDAL_NODATA `{text}` is not a number")))?,
        ),
    };

    Ok(Raster {
        width,
        height,
        origin_top_left,
        pixel: [scale[0], scale[1]],
        values,
        nodata,
    })
}

/// The lowest a relief value may honestly be, in metres: the Dead Sea shore,
/// the lowest dry land on Earth, is at about -430 m. Anything under this is not
/// a height, whatever the file declared. Pinned by `tests/terrain.rs`.
pub const LOWEST_LAND_M: f64 = -500.0;

/// How far a resampled answer's contamination reaches around a no-data pixel,
/// in pixels. **Measured** (IGN `HIGHRES.MNS` over Paris, 500 x 500):
/// after one ring a pixel at -477 m was still left, after two the lowest
/// survivor was 19 m. A cubic kernel reaches two pixels.
pub const BLEND_REACH: usize = 2;

/// What [`voids_of`] found, and how: three different reasons for a hole, kept
/// apart so that a face can say which one it applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Voids {
    /// Pixels equal to the file's own `GDAL_NODATA` (compared as `f32`, the type
    /// the values were stored in), plus any NaN or infinite value.
    pub declared: usize,
    /// Pixels below [`LOWEST_LAND_M`] that the file did **not** declare. The IGN
    /// surface model declares `-99999` and writes `-9999`.
    pub undeclared: usize,
    /// Pixels the service **resampled** across a no-data one: a bilinear or
    /// cubic blend of `-9999` and a real height is a fractional number, such as
    /// `-8498.93`, and its neighbours are contaminated too. When any such value
    /// is seen, every pixel within [`BLEND_REACH`] of a hole is a hole as well;
    /// this counts the ones that were not already.
    pub blended_ring: usize,
}

impl Voids {
    /// Every pixel that is a hole.
    pub fn total(&self) -> usize {
        self.declared + self.undeclared + self.blended_ring
    }
}

/// Which pixels of a raster have no height - row-major, first row north, like
/// [`Raster::values`] - and why.
///
/// A relief raster has no height under [`LOWEST_LAND_M`], so that is a hole
/// whatever the file says; what the file declares is a hole too, and a resampled
/// service's blends are holes around them. Nothing here is silent: the counts of
/// [`Voids`] are what a face reports.
pub fn voids_of(raster: &Raster) -> (Vec<bool>, Voids) {
    let (w, h) = (raster.width, raster.height);
    let declared = raster.nodata.map(|n| n as f32);
    let mut hole = vec![false; raster.values.len()];
    let mut voids = Voids::default();
    let mut blended = false;
    for (flag, &v) in hole.iter_mut().zip(&raster.values) {
        if !v.is_finite() || declared.is_some_and(|n| v as f32 == n) {
            *flag = true;
            voids.declared += 1;
        } else if v < LOWEST_LAND_M {
            *flag = true;
            voids.undeclared += 1;
            // A sentinel is a whole number; a blend of one with a height is not.
            blended |= v.fract() != 0.0;
        }
    }
    if blended && w > 0 && h > 0 {
        let seed = hole.clone();
        for row in 0..h {
            for col in 0..w {
                if hole[row * w + col] {
                    continue;
                }
                let near = (row.saturating_sub(BLEND_REACH)..=(row + BLEND_REACH).min(h - 1)).any(|r| {
                    (col.saturating_sub(BLEND_REACH)..=(col + BLEND_REACH).min(w - 1))
                        .any(|c| seed[r * w + c])
                });
                if near {
                    hole[row * w + col] = true;
                    voids.blended_ring += 1;
                }
            }
        }
    }
    (hole, voids)
}

/// A raster as the house lattice: `z[j*ni + i]`, `j` towards the **north**,
/// origin at the **south-west pixel centre**.
///
/// Two moves, both pinned by a test on an asymmetric raster: the corner
/// tiepoint becomes a centre (half a pixel in, on both axes), and the rows are
/// flipped because a GeoTIFF's first row is the north edge. Every pixel
/// [`voids_of`] finds without a height becomes NaN, which is how a lattice
/// writes a hole (RESQML's null for a double is NaN).
pub fn lattice_of(raster: &Raster) -> Result<energyml_build::pieces::Lattice> {
    let (w, h) = (raster.width, raster.height);
    if w == 0 || h == 0 {
        return Err(bad("the raster has no pixels"));
    }
    if raster.values.len() != w * h {
        return Err(bad(format!(
            "{} values for a {w}x{h} raster",
            raster.values.len()
        )));
    }
    if !(raster.pixel[0] > 0.0 && raster.pixel[1] > 0.0) {
        return Err(bad("the pixel size is not positive"));
    }
    let (hole, _) = voids_of(raster);
    let mut z = Vec::with_capacity(w * h);
    for j in 0..h {
        let row = h - 1 - j; // j counts from the south; the file counts from the north
        for i in 0..w {
            z.push(if hole[row * w + i] {
                f64::NAN
            } else {
                raster.values[row * w + i]
            });
        }
    }
    Ok(energyml_build::pieces::Lattice {
        origin: [
            raster.origin_top_left[0] + raster.pixel[0] / 2.0,
            raster.origin_top_left[1] - (h as f64 - 0.5) * raster.pixel[1],
        ],
        spacing: raster.pixel,
        counts: [w, h],
        z,
    })
}

// ---------------------------------------------------------------------------
// Asking for a terrain window - what a face calls, so that the CLI and the
// front end decide nothing (ADR-0012: no face carries logic).

/// A terrain window, ready to be asked for: which source, the window snapped to
/// whole pixels of the step, and the request. Computed, never fetched.
#[derive(Debug, Clone)]
pub struct Ask<'a> {
    /// The source chosen - the one named, or the finest that covers the area.
    pub source: &'a TerrainSource,
    /// The window actually asked for, `[x_min, y_min, x_max, y_max]` in `epsg`:
    /// the area given, its east and north edges pushed out to whole pixels.
    pub bbox: [f64; 4],
    /// The system of `bbox` and of the answer.
    pub epsg: u32,
    /// Pixels along x.
    pub width: u32,
    /// Pixels along y.
    pub height: u32,
    /// The ground size of a pixel, the step asked for.
    pub step: f64,
    /// The request.
    pub request: Request,
}

impl Ask<'_> {
    /// The service the source is spoken to at - what the `terrain` operation
    /// records as `service`.
    pub fn service(&self) -> &str {
        match &self.source.protocol {
            Protocol::WmsRaster { url, .. } | Protocol::Wmts { url, .. } | Protocol::Cog { url } => url,
            Protocol::XyzTerrarium { template } => template,
        }
    }

    /// The layer asked of it - what the operation records as `layer`; the
    /// source id when the protocol has no layer.
    pub fn layer(&self) -> &str {
        match &self.source.protocol {
            Protocol::WmsRaster { layer, .. } | Protocol::Wmts { layer, .. } => layer,
            _ => &self.source.id,
        }
    }
}

/// Why no terrain can be made from `source` by this crate today, or `None`.
///
/// The one verdict, used by [`ask`] when it chooses on the caller's behalf and
/// by a face that greys what cannot be chosen - two copies of *which sources
/// are read* would disagree the day a protocol gains its reader.
pub fn unread(source: &TerrainSource) -> Option<String> {
    if source.kind != SourceKind::Relief {
        return Some(format!(
            "a source of {}, and a terrain is relief",
            source.kind.label()
        ));
    }
    if source.needs_key {
        return Some("this source needs a key, and nothing here sends one".to_string());
    }
    match &source.protocol {
        Protocol::WmsRaster { .. } => None,
        Protocol::Wmts { .. } => Some("speaks wmts, which is not read yet".to_string()),
        Protocol::XyzTerrarium { .. } => Some("speaks xyzTerrarium, which is not read yet".to_string()),
        Protocol::Cog { .. } => Some("speaks cog, which is not read yet".to_string()),
    }
}

/// Prepare a relief window: choose the source, snap the window, build the
/// request.
///
/// - `source` names a catalogue entry; left out, **the finest relief source
///   whose protocol is read** and whose coverage holds `area_wgs84` is taken -
///   a source this module cannot read yet is never chosen on the caller's
///   behalf;
/// - `area` is `[x_min, y_min, x_max, y_max]` in `epsg`; `area_wgs84` is the
///   same area in degrees, for the coverage (the caller reprojects: geodesy
///   lives in `energyml-proj`, not here);
/// - `step` is the pixel size in the units of `epsg`. The window keeps its
///   south-west corner and grows to whole pixels: an area 1 000 m wide at a
///   step of 300 m is asked as 1 200 m, said by `bbox`, never cut short.
///
/// Refused by name: a step that is not a positive number, an unknown source,
/// a source that needs a key (nothing here sends one),
/// a source of another kind, a source that does not cover the area, and a
/// window above [`MAX_PIXELS`] on a side - with the step that would fit.
pub fn ask<'a>(
    catalogue: &'a TerrainCatalogue,
    source: Option<&str>,
    area: [f64; 4],
    epsg: u32,
    area_wgs84: [f64; 4],
    step: f64,
) -> Result<Ask<'a>> {
    if !(step.is_finite() && step > 0.0) {
        return Err(Error::Refused(format!(
            "the step must be a positive length, not {step}"
        )));
    }
    if !area.iter().all(|v| v.is_finite()) || area[0] >= area[2] || area[1] >= area[3] {
        return Err(Error::Refused(format!(
            "the area {area:?} is not [x_min, y_min, x_max, y_max] with min < max"
        )));
    }
    let chosen = match source {
        Some(id) => {
            let Some(found) = catalogue.get(id) else {
                let known: Vec<&str> = catalogue.sources().iter().map(|one| one.id.as_str()).collect();
                return Err(Error::Refused(format!(
                    "no source `{id}` in the catalogue; it holds {}",
                    known.join(", ")
                )));
            };
            if found.needs_key {
                return Err(Error::Refused(format!(
                    "`{id}` needs a key, and nothing here sends one"
                )));
            }
            if found.kind != SourceKind::Relief {
                return Err(Error::Refused(format!(
                    "`{id}` is a source of {}, and a terrain is relief",
                    found.kind.label()
                )));
            }
            if !found.coverage.contains(area_wgs84) {
                return Err(Error::Refused(format!(
                    "`{id}` does not cover the area [west {}, south {}, east {}, north {}] (WGS 84 degrees); its coverage is {:?}",
                    area_wgs84[0], area_wgs84[1], area_wgs84[2], area_wgs84[3], found.coverage.boxes
                )));
            }
            found
        }
        None => {
            let covering = catalogue.covering(SourceKind::Relief, area_wgs84);
            if covering.is_empty() {
                return Err(catalogue.refusal(SourceKind::Relief, area_wgs84));
            }
            // Neither a protocol not read yet nor a source that needs a key is
            // chosen on the caller's behalf: the first would fail, the second
            // would be refused only once chosen, over a source that would do.
            match covering.iter().find(|one| unread(one).is_none()) {
                Some(found) => *found,
                None => {
                    let ids: Vec<&str> = covering.iter().map(|one| one.id.as_str()).collect();
                    return Err(Error::Refused(format!(
                        "the sources covering this area ({}) speak protocols that are not read yet or need a key; name one with a wmsRaster protocol and no key",
                        ids.join(", ")
                    )));
                }
            }
        }
    };
    let cells = |extent: f64| (extent / step).ceil().max(1.0);
    let (along_x, along_y) = (cells(area[2] - area[0]), cells(area[3] - area[1]));
    if along_x > f64::from(MAX_PIXELS) || along_y > f64::from(MAX_PIXELS) {
        let fits = ((area[2] - area[0]).max(area[3] - area[1]) / f64::from(MAX_PIXELS)).ceil();
        return Err(Error::Refused(format!(
            "a step of {step} makes a window of {along_x} x {along_y} pixels, past the {MAX_PIXELS} the service answers on a side; a step of {fits} or more fits in one request"
        )));
    }
    let (width, height) = (along_x as u32, along_y as u32);
    let bbox = [
        area[0],
        area[1],
        area[0] + along_x * step,
        area[1] + along_y * step,
    ];
    let request = wms_raster_request(chosen, bbox, epsg, width, height)?;
    Ok(Ask {
        source: chosen,
        bbox,
        epsg,
        width,
        height,
        step,
        request,
    })
}

/// Read what a terrain request answered: a GeoTIFF, or the service's own
/// explanation.
///
/// A WMS that refuses answers **in XML, often with a 200** - a
/// `ServiceException` - and reading it as a TIFF would say *not a TIFF*, true
/// and useless. So anything that is not a TIFF (by its first bytes, not by the
/// declared type, which a refusal does not always set) is read as text and
/// handed back as the server's words.
pub fn read_answer(answer: crate::transport::Answer, most: usize) -> Result<Raster> {
    use std::io::Read;
    let status = answer.status;
    let mut body = Vec::new();
    let read = answer
        .body
        .take(most as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|why| Error::Transport(format!("reading the answer: {why}")))?;
    if read > most {
        return Err(Error::TooMuch(format!(
            "the answer is longer than {most} bytes; ask a coarser step or a smaller area"
        )));
    }
    let is_tiff = body.starts_with(b"II*\0") || body.starts_with(b"MM\0*");
    if status != 200 || !is_tiff {
        let says: String = String::from_utf8_lossy(&body).chars().take(2000).collect();
        return Err(Error::Server {
            status,
            code: None,
            says: says.trim().to_string(),
        });
    }
    read_geotiff(&body)
}

// ---------------------------------------------------------------------------
// What a face needs besides the request - so that no face reprojects corners
// or spells a tile address (ADR-0012: no face carries logic).

/// An area in WGS 84 degrees, `[west, south, east, north]`: the four corners of
/// `area` (`[x_min, y_min, x_max, y_max]` in `epsg`) moved by `reproject` and
/// enveloped, so that a box the projection turns is not cut short.
///
/// The geodesy is the caller's - `energyml-proj` natively and in wasm alike -
/// because this crate does no geodesy; what is decided here is *which points*
/// and *which envelope*, once for the CLI and the front end. `reproject` takes and
/// gives `[x, y, z]`, longitude first for a geographic system.
pub fn area_wgs84(
    area: [f64; 4],
    epsg: u32,
    reproject: impl Fn([f64; 3]) -> std::result::Result<[f64; 3], String>,
) -> Result<[f64; 4]> {
    if !area.iter().all(|v| v.is_finite()) || area[0] >= area[2] || area[1] >= area[3] {
        return Err(Error::Refused(format!(
            "the area {area:?} is not [x_min, y_min, x_max, y_max] with min < max"
        )));
    }
    if epsg == 4326 {
        return Ok(area);
    }
    let corners = [
        [area[0], area[1]],
        [area[2], area[1]],
        [area[0], area[3]],
        [area[2], area[3]],
    ];
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for [x, y] in corners {
        let p =
            reproject([x, y, 0.0]).map_err(|why| Error::Refused(format!("EPSG:{epsg} to WGS 84: {why}")))?;
        if !(p[0].is_finite() && p[1].is_finite()) {
            return Err(Error::Refused(format!(
                "EPSG:{epsg} to WGS 84: the corner ({x}, {y}) has no position in degrees"
            )));
        }
        out = [
            out[0].min(p[0]),
            out[1].min(p[1]),
            out[2].max(p[0]),
            out[3].max(p[1]),
        ];
    }
    Ok(out)
}

/// The `{z}/{x}/{y}` address of an imagery source, for a tile layer.
///
/// Only a WMTS in **`PM`** (Web Mercator, the matrix set every tile map
/// speaks) is a template: its `TILEMATRIX` is the zoom, `TILEROW` the `y` and
/// `TILECOL` the `x` - measured on the IGN Géoplateforme.
/// Another matrix set would need its own grid, and is
/// refused by name rather than drawn at the wrong place; another protocol is
/// not imagery a tile layer can take.
pub fn tile_template(source: &TerrainSource) -> Result<String> {
    let Protocol::Wmts {
        url,
        layer,
        tile_matrix_set,
        format,
        style,
    } = &source.protocol
    else {
        return Err(Error::Refused(format!(
            "source `{}` is not a WMTS, so it has no tile address; only a WMTS in PM is drawn as a basemap",
            source.id
        )));
    };
    if tile_matrix_set != "PM" {
        return Err(Error::Refused(format!(
            "source `{}` tiles in `{tile_matrix_set}`; only `PM` (Web Mercator) is drawn as a basemap",
            source.id
        )));
    }
    Ok(format!(
        "{url}?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER={}&STYLE={}&TILEMATRIXSET=PM&TILEMATRIX={{z}}&TILEROW={{y}}&TILECOL={{x}}&FORMAT={}",
        encode(layer),
        encode(style),
        encode(format),
    ))
}

/// The whole `N`-item chunks of `items`, as arrays (what `<[T]>::as_chunks` gives);
/// a trailing remainder is dropped.
fn fixed_chunks<T, const N: usize>(items: &[T]) -> impl Iterator<Item = &[T; N]> {
    items.as_chunks::<N>().0.iter()
}
