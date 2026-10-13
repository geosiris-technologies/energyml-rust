//! Terrain sources, without a network and without a file outside the repository.
//!
//! The GeoTIFFs here are **fabricated byte by byte** by `Tiff` below, following
//! the TIFF 6.0 baseline layout (Adobe, 1992, section 2 "TIFF Structure" for
//! the header and IFD, section 4 for `StripOffsets`) and the GeoTIFF 1.0 tags
//! 33550 and 33922. The shape of the one the IGN service answers was measured
//! (little-endian, uncompressed, one strip, float32,
//! `GDAL_NODATA` "-99999") and is only the *form* the fabricated bytes take.

use energyml_providers::error::Error;
use energyml_providers::terrain;
use energyml_providers::terrain::{
    MAX_PIXELS, Protocol, SourceKind, TerrainCatalogue, lat_lon_first, lattice_of, read_geotiff, voids_of,
    wms_raster_request,
};

fn builtin() -> TerrainCatalogue {
    TerrainCatalogue::builtin().expect("the embedded catalogue parses")
}

fn message(error: Error) -> String {
    error.to_string()
}

// ---------------------------------------------------------------- catalogue

/// To make it fail: remove the licence or the attribution of a terrain.json entry.
#[test]
fn the_builtin_catalogue_parses_and_every_entry_carries_licence_and_attribution() {
    let catalogue = builtin();
    assert!(catalogue.sources().len() >= 7, "the seven entries shipped");
    for source in catalogue.sources() {
        assert!(!source.licence.trim().is_empty(), "{} has no licence", source.id);
        assert!(
            !source.attribution.trim().is_empty(),
            "{} has no attribution",
            source.id
        );
        assert!(source.resolution_m > 0.0, "{} has no resolution", source.id);
        assert!(!source.crs.is_empty(), "{} lists no CRS", source.id);
    }
    let rge = catalogue.get("ign-rge-alti").expect("RGE ALTI is there");
    assert!(
        matches!(&rge.protocol, Protocol::WmsRaster { layer, .. } if layer == "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES")
    );
    assert!(!rge.needs_key);
}

/// To make it fail: remove from `parse` the check for duplicated ids or impossible boxes.
#[test]
fn a_catalogue_with_a_duplicated_id_or_an_impossible_box_is_refused_by_name() {
    let dup = r#"{"sources":[
        {"id":"a","title":"t","kind":"relief","protocol":{"type":"cog","url":"u"},
         "coverage":{"boxes":[[0,0,1,1]]},"crs":[4326],"resolutionM":1,"licence":"l","attribution":"x","needsKey":false},
        {"id":"a","title":"t","kind":"relief","protocol":{"type":"cog","url":"u"},
         "coverage":{"boxes":[[0,0,1,1]]},"crs":[4326],"resolutionM":1,"licence":"l","attribution":"x","needsKey":false}]}"#;
    assert!(message(TerrainCatalogue::parse(dup).unwrap_err()).contains("`a` appears twice"));

    let flat = format!(r#"{{"sources":[{}]}}"#, one("flat", 1.0, "[[5,0,1,1]]"));
    assert!(message(TerrainCatalogue::parse(&flat).unwrap_err()).contains("impossible coverage box"));

    assert!(message(TerrainCatalogue::parse("not json").unwrap_err()).contains("terrain catalogue"));
}

fn one(id: &str, resolution: f64, boxes: &str) -> String {
    format!(
        r#"{{"id":"{id}","title":"mine","kind":"relief","protocol":{{"type":"cog","url":"u"}},
        "coverage":{{"boxes":{boxes}}},"crs":[4326],"resolutionM":{resolution},
        "licence":"l","attribution":"a","needsKey":false}}"#
    )
}

/// To make it fail: a `with_user` that keeps the shipped entry beside the user's, or that puts it before.
#[test]
fn a_user_entry_replaces_the_builtin_with_the_same_id_and_is_tried_first() {
    let user = TerrainCatalogue::parse(&format!(
        r#"{{"sources":[{}, {}]}}"#,
        one("ign-rge-alti", 7.0, "[[0,0,1,1]]"),
        one("mine-too", 30.0, "[[0,0,1,1]]"),
    ))
    .unwrap();
    let before = builtin().sources().len();
    let merged = builtin().with_user(user);

    // replaced, not duplicated: one new id, one replaced id
    assert_eq!(merged.sources().len(), before + 1);
    assert_eq!(
        merged.sources().iter().filter(|s| s.id == "ign-rge-alti").count(),
        1
    );
    let rge = merged.get("ign-rge-alti").unwrap();
    assert_eq!(rge.resolution_m, 7.0, "the user's entry wins");
    assert_eq!(rge.title, "mine");

    // tried first: user entries lead, the rest follow in their old order
    assert_eq!(merged.sources()[0].id, "ign-rge-alti");
    assert_eq!(merged.sources()[1].id, "mine-too");

    // and a tie on resolution is won by the user's entry
    let tie = TerrainCatalogue::parse(&format!(
        r#"{{"sources":[{}]}}"#,
        one("user-30", 30.0, "[[-180,-90,180,90]]")
    ))
    .unwrap();
    let merged = builtin().with_user(tie);
    let found = merged.covering(SourceKind::Relief, [100.0, -10.0, 100.5, -9.5]);
    assert_eq!(found[0].id, "user-30", "same 30 m as Copernicus, user first");
}

/// To make it fail: sort `covering` by catalogue order instead of resolution, or in descending order.
#[test]
fn covering_ranks_by_resolution_and_a_covered_box_is_not_a_partial_one() {
    let catalogue = builtin();
    // Central Paris: inside every French relief source and the world ones.
    let paris = [2.30, 48.84, 2.36, 48.88];
    let ids: Vec<&str> = catalogue
        .covering(SourceKind::Relief, paris)
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        ids.first().copied(),
        Some("ign-lidar-hd-mnt"),
        "0.5 m first: {ids:?}"
    );
    let resolutions: Vec<f64> = catalogue
        .covering(SourceKind::Relief, paris)
        .iter()
        .map(|s| s.resolution_m)
        .collect();
    assert!(
        resolutions.windows(2).all(|w| w[0] <= w[1]),
        "ascending metres: {resolutions:?}"
    );
    assert!(ids.contains(&"copernicus-dem-30"));

    // Only Imagery sources are offered for Imagery.
    assert!(
        catalogue
            .covering(SourceKind::Imagery, paris)
            .iter()
            .all(|s| s.kind == SourceKind::Imagery)
    );

    // A box that sticks out of metropolitan France is not covered by the
    // metropolitan LiDAR HD, though it overlaps it.
    let across = [9.0, 45.0, 12.0, 46.0];
    let ids: Vec<&str> = catalogue
        .covering(SourceKind::Relief, across)
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert!(!ids.contains(&"ign-lidar-hd-mnt"), "{ids:?}");
}

/// To make it fail: a `refusal` that does not name the area, or that does not list the sources considered.
#[test]
fn nothing_covering_an_area_is_empty_and_the_refusal_names_the_area_and_the_sources() {
    let only_france = TerrainCatalogue::parse(&format!(
        r#"{{"sources":[{}]}}"#,
        one("france-only", 1.0, "[[-5.5,41.0,10.0,51.2]]")
    ))
    .unwrap();
    let brazil = [-47.9, -15.9, -47.8, -15.8];
    assert!(only_france.covering(SourceKind::Relief, brazil).is_empty());
    let why = message(only_france.refusal(SourceKind::Relief, brazil));
    assert!(
        why.contains("-47.9") && why.contains("-15.8"),
        "names the area: {why}"
    );
    assert!(why.contains("france-only"), "names what was considered: {why}");

    // No building source at all: said as such, not as an empty list.
    let catalogue = builtin();
    assert!(
        catalogue
            .covering(SourceKind::Buildings, [2.3, 48.8, 2.4, 48.9])
            .is_empty()
    );
    let why = message(catalogue.refusal(SourceKind::Buildings, [2.3, 48.8, 2.4, 48.9]));
    assert!(why.contains("no buildings source at all"), "{why}");
}

// -------------------------------------------------------------- WMS request

/// Percent-decode, enough for what `encode` writes.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut k = 0;
    while k < bytes.len() {
        if bytes[k] == b'%' {
            out.push(u8::from_str_radix(&text[k + 1..k + 3], 16).unwrap());
            k += 3;
        } else {
            out.push(bytes[k]);
            k += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn query_of(url: &str) -> std::collections::HashMap<String, String> {
    let (_, query) = url.split_once('?').expect("a query string");
    query
        .split('&')
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (k.to_string(), decode(v))
        })
        .collect()
}

fn numbers(list: &str) -> Vec<f64> {
    list.split(',').map(|n| n.parse().unwrap()).collect()
}

/// To make it fail: write the EPSG:4326 BBOX as x,y, or the EPSG:2154 one as y,x.
#[test]
fn the_bbox_axis_order_follows_the_crs_projected_xy_geographic_latlon() {
    let catalogue = builtin();
    let rge = catalogue.get("ign-rge-alti").unwrap();

    // Projected: easting, northing — the window the service was measured with.
    let request = wms_raster_request(rge, [651000.0, 6860000.0, 652000.0, 6861000.0], 2154, 20, 20).unwrap();
    let q = query_of(&request.url);
    assert_eq!(q["CRS"], "EPSG:2154");
    assert_eq!(
        numbers(&q["BBOX"]),
        vec![651000.0, 6860000.0, 652000.0, 6861000.0]
    );
    assert_eq!(q["VERSION"], "1.3.0");
    assert_eq!(q["REQUEST"], "GetMap");
    assert_eq!(q["LAYERS"], "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES");
    assert_eq!(q["FORMAT"], "image/geotiff");
    assert_eq!((q["WIDTH"].as_str(), q["HEIGHT"].as_str()), ("20", "20"));
    assert!(request.url.starts_with("https://data.geopf.fr/wms-r?"));

    // Geographic: the caller passes lon,lat order; the wire carries lat,lon.
    let request = wms_raster_request(rge, [2.0, 48.0, 2.1, 48.1], 4326, 10, 10).unwrap();
    let q = query_of(&request.url);
    assert_eq!(q["CRS"], "EPSG:4326");
    assert_eq!(
        numbers(&q["BBOX"]),
        vec![48.0, 2.0, 48.1, 2.1],
        "WMS 1.3.0, EPSG:4326: lat,lon"
    );

    assert!(lat_lon_first(4326) && !lat_lon_first(2154));
}

/// To make it fail: remove the 5010 ceiling or accept an EPSG the source does not declare.
#[test]
fn refusals_before_any_request_size_crs_window_and_unread_protocols() {
    let catalogue = builtin();
    let rge = catalogue.get("ign-rge-alti").unwrap();
    let window = [651000.0, 6860000.0, 652000.0, 6861000.0];

    assert!(
        wms_raster_request(rge, window, 2154, MAX_PIXELS, MAX_PIXELS).is_ok(),
        "5010 is the limit, accepted"
    );
    assert_eq!(MAX_PIXELS, 5010);
    assert!(message(wms_raster_request(rge, window, 2154, 5011, 10).unwrap_err()).contains("5010"));
    assert!(message(wms_raster_request(rge, window, 2154, 10, 5011).unwrap_err()).contains("5010"));
    assert!(wms_raster_request(rge, window, 2154, 0, 10).is_err());

    let why = message(wms_raster_request(rge, window, 3857, 10, 10).unwrap_err());
    assert!(why.contains("EPSG:3857") && why.contains("ign-rge-alti"), "{why}");

    // an inverted window is not silently swapped
    assert!(wms_raster_request(rge, [652000.0, 6861000.0, 651000.0, 6860000.0], 2154, 10, 10).is_err());

    // Protocols with no builder: refused by name, never an empty answer.
    for (id, name) in [
        ("copernicus-dem-30", "cog"),
        ("aws-terrain-tiles", "xyzTerrarium"),
        ("ign-ortho", "wmts"),
    ] {
        let source = catalogue.get(id).unwrap();
        let epsg = source.crs[0];
        let why = message(wms_raster_request(source, [0.0, 0.0, 1.0, 1.0], epsg, 10, 10).unwrap_err());
        assert!(why.contains("not read yet") && why.contains(name), "{id}: {why}");
    }
}

// ------------------------------------------------------------------ GeoTIFF

/// A little-endian baseline TIFF builder. Each field is a way to be wrong.
struct Tiff {
    width: usize,
    height: usize,
    values: Vec<f32>,
    rows_per_strip: usize,
    compression: u16,
    bits: u16,
    sample_format: u16,
    samples: u16,
    tiled: bool,
    georef: bool,
    nodata: Option<&'static str>,
    point_is_area: bool,
}

impl Tiff {
    fn new(width: usize, height: usize, values: Vec<f32>) -> Self {
        assert_eq!(values.len(), width * height);
        Tiff {
            width,
            height,
            values,
            rows_per_strip: height,
            compression: 1,
            bits: 32,
            sample_format: 3,
            samples: 1,
            tiled: false,
            georef: true,
            nodata: Some("-99999"),
            point_is_area: true,
        }
    }

    fn build(&self) -> Vec<u8> {
        // (tag, field type, count, raw little-endian bytes)
        type Field = (u16, u16, u32, Vec<u8>);
        let mut out: Vec<u8> = vec![b'I', b'I', 42, 0, 0, 0, 0, 0];

        let mut offsets: Vec<u32> = Vec::new();
        let mut counts: Vec<u32> = Vec::new();
        let mut row = 0;
        while row < self.height {
            let rows = self.rows_per_strip.min(self.height - row);
            offsets.push(out.len() as u32);
            for v in &self.values[row * self.width..(row + rows) * self.width] {
                out.extend(v.to_le_bytes());
            }
            counts.push((rows * self.width * 4) as u32);
            row += rows;
        }

        let short = |tag: u16, v: &[u16]| -> Field {
            (
                tag,
                3,
                v.len() as u32,
                v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            )
        };
        let long = |tag: u16, v: &[u32]| -> Field {
            (
                tag,
                4,
                v.len() as u32,
                v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            )
        };
        let doubles = |tag: u16, v: &[f64]| -> Field {
            (
                tag,
                12,
                v.len() as u32,
                v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            )
        };

        let mut fields: Vec<Field> = vec![
            long(256, &[self.width as u32]),
            long(257, &[self.height as u32]),
            short(258, &[self.bits]),
            short(259, &[self.compression]),
            short(262, &[1]),
            long(273, &offsets),
            short(277, &[self.samples]),
            long(278, &[self.rows_per_strip as u32]),
            long(279, &counts),
            short(339, &[self.sample_format]),
        ];
        if self.tiled {
            fields.push(long(322, &[16]));
            fields.push(long(323, &[16]));
        }
        if self.georef {
            // Pixel 10 m east, 20 m south; top-left corner at (1000, 2000).
            fields.push(doubles(33550, &[10.0, 20.0, 0.0]));
            fields.push(doubles(33922, &[0.0, 0.0, 0.0, 1000.0, 2000.0, 0.0]));
            // GeoKeyDirectory: header (1, 1, 0, 1 key), then GTRasterTypeGeoKey.
            let raster_type = if self.point_is_area { 1 } else { 2 };
            fields.push(short(34735, &[1, 1, 0, 1, 1025, 0, 1, raster_type]));
        }
        if let Some(text) = self.nodata {
            let mut raw = text.as_bytes().to_vec();
            raw.push(0);
            fields.push((42113, 2, raw.len() as u32, raw));
        }
        fields.sort_by_key(|f| f.0);

        // Values over four bytes go after the strips; the IFD goes last.
        let mut table: Vec<[u8; 12]> = Vec::new();
        for (tag, ty, count, raw) in &fields {
            let mut cell = [0u8; 12];
            cell[0..2].copy_from_slice(&tag.to_le_bytes());
            cell[2..4].copy_from_slice(&ty.to_le_bytes());
            cell[4..8].copy_from_slice(&count.to_le_bytes());
            if raw.len() <= 4 {
                cell[8..8 + raw.len()].copy_from_slice(raw);
            } else {
                if out.len() % 2 == 1 {
                    out.push(0);
                }
                cell[8..12].copy_from_slice(&(out.len() as u32).to_le_bytes());
                out.extend(raw);
            }
            table.push(cell);
        }
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let ifd_at = out.len() as u32;
        out.extend((table.len() as u16).to_le_bytes());
        for cell in &table {
            out.extend(cell);
        }
        out.extend(0u32.to_le_bytes());
        out[4..8].copy_from_slice(&ifd_at.to_le_bytes());
        out
    }
}

const NODATA: f32 = -99999.0;

/// 3 wide, 2 high; the north row is first. Asymmetric on both axes.
fn asymmetric() -> Tiff {
    Tiff::new(3, 2, vec![1.0, 2.0, 3.0, 4.0, NODATA, 6.0])
}

/// To make it fail: read a band other than the first row as north, or lose the origin, the step or the nodata.
#[test]
fn a_fabricated_float32_geotiff_reads_back_with_its_values_origin_and_nodata() {
    let raster = read_geotiff(&asymmetric().build()).unwrap();
    assert_eq!((raster.width, raster.height), (3, 2));
    assert_eq!(
        raster.values,
        vec![1.0, 2.0, 3.0, 4.0, -99999.0, 6.0],
        "file order, north row first"
    );
    assert_eq!(raster.origin_top_left, [1000.0, 2000.0]);
    assert_eq!(raster.pixel, [10.0, 20.0]);
    assert_eq!(raster.nodata, Some(-99999.0));
}

/// To make it fail: read only the first band of rows of a file with several strips.
#[test]
fn several_strips_read_like_one() {
    let mut tiff = asymmetric();
    tiff.rows_per_strip = 1;
    let several = read_geotiff(&tiff.build()).unwrap();
    let one = read_geotiff(&asymmetric().build()).unwrap();
    assert_eq!(several, one);

    // an odd split: 5 rows, 2 per strip -> strips of 2, 2 and 1 rows
    let values: Vec<f32> = (0..10).map(|v| v as f32).collect();
    let mut tiff = Tiff::new(2, 5, values.clone());
    tiff.rows_per_strip = 2;
    let raster = read_geotiff(&tiff.build()).unwrap();
    assert_eq!(
        raster.values,
        values.iter().map(|&v| v as f64).collect::<Vec<_>>()
    );
}

/// To make it fail: do not flip the rows, or forget the half pixel (origin = corner instead of centre).
#[test]
fn the_lattice_flips_the_rows_and_puts_the_origin_at_the_south_west_pixel_centre() {
    let raster = read_geotiff(&asymmetric().build()).unwrap();
    let lattice = lattice_of(&raster).unwrap();

    assert_eq!(lattice.counts, [3, 2]);
    assert_eq!(lattice.spacing, [10.0, 20.0]);
    // Corner (1000, 2000) is the NW corner; the two rows span y 2000 -> 1960.
    // The south-west pixel centre is x = 1000 + 5, y = 1960 + 10.
    assert_eq!(lattice.origin, [1005.0, 1970.0]);

    // j = 0 is the SOUTH row of the file (its last), j = 1 the north one.
    let (south, north) = lattice.z.split_at(3);
    assert_eq!(north, &[1.0, 2.0, 3.0], "the file's first row ends up last");
    assert_eq!(south[0], 4.0);
    assert_eq!(south[2], 6.0);
    // The top-left pixel of the file is node (0, nj-1): its centre is
    // origin + (0, (nj-1)*dy) = (1005, 1990) = corner + (5, -10).
    assert_eq!(lattice.origin[1] + 1.0 * lattice.spacing[1], 1990.0);
}

/// To make it fail: leave -99999 in the heights, or turn into NaN a value that is not the nodata.
#[test]
fn nodata_becomes_nan_and_only_nodata() {
    let lattice = lattice_of(&read_geotiff(&asymmetric().build()).unwrap()).unwrap();
    assert!(lattice.z[1].is_nan(), "the nodata pixel, south row, middle");
    assert_eq!(lattice.z.iter().filter(|v| v.is_nan()).count(), 1);
    assert_eq!(lattice.z[0], 4.0);

    // No GDAL_NODATA: nothing is *declared*, but -99999 is still no height.
    let mut tiff = asymmetric();
    tiff.nodata = None;
    let raster = read_geotiff(&tiff.build()).unwrap();
    assert_eq!(raster.nodata, None);
    let (_, voids) = voids_of(&raster);
    assert_eq!((voids.declared, voids.undeclared), (0, 1));
    assert!(lattice_of(&raster).unwrap().z[1].is_nan());
}

/// 9 x 9 of 50 m with the centre pixel (4, 4) set; the rest is `fill`.
fn field(centre: f32, edits: &[((usize, usize), f32)]) -> Tiff {
    let mut values = vec![50.0f32; 81];
    values[4 * 9 + 4] = centre;
    for ((r, c), v) in edits {
        values[r * 9 + c] = *v;
    }
    Tiff::new(9, 9, values)
}

/// Rule (relief raster): a value that the file declares as no data is a hole,
/// and so is one it writes without declaring it. Measured on the IGN
/// `HIGHRES.MNS` layer: `GDAL_NODATA` says -99999, the pixels hold -9999.
/// To make it fail: remove the `v < LOWEST_LAND_M` branch from `voids_of`, or
/// compare against the declared `nodata` alone.
#[test]
fn a_sentinel_the_file_does_not_declare_is_a_hole_and_is_counted_apart() {
    let mut tiff = field(-9999.0, &[]); // declares "-99999"
    let raster = read_geotiff(&tiff.build()).unwrap();
    assert_eq!(raster.nodata, Some(-99999.0));
    let (hole, voids) = voids_of(&raster);
    assert_eq!((voids.declared, voids.undeclared, voids.blended_ring), (0, 1, 0));
    assert!(hole[4 * 9 + 4]);
    let lattice = lattice_of(&raster).unwrap();
    assert_eq!(lattice.z.iter().filter(|v| v.is_nan()).count(), 1);
    assert!(
        lattice.z.iter().all(|v| v.is_nan() || *v == 50.0),
        "no -9999 is left as a height"
    );

    // The declared one is counted as declared.
    tiff = field(-99999.0, &[]);
    let (_, voids) = voids_of(&read_geotiff(&tiff.build()).unwrap());
    assert_eq!((voids.declared, voids.undeclared), (1, 0));
}

/// Rule: the comparison is made in the type the values were stored in. A
/// declared `-3.4e38` parses as an `f64` that no `f32` widened to `f64` equals.
/// To make it fail: compare `v == nodata` as `f64` in `voids_of`.
#[test]
fn a_declared_value_is_compared_as_the_float32_it_was_stored_as() {
    let mut tiff = field(-3.4e38_f32, &[]);
    tiff.nodata = Some("-3.4e38");
    let raster = read_geotiff(&tiff.build()).unwrap();
    assert_ne!(
        raster.values[4 * 9 + 4],
        -3.4e38_f64,
        "the premise: a strict f64 comparison misses it"
    );
    let (hole, voids) = voids_of(&raster);
    assert!(hole[4 * 9 + 4]);
    assert_eq!(voids.declared, 1);
}

/// Rule: a service that resamples blends the sentinel with real heights (the IGN
/// answer held -9999.168, -8498.93, -477.46 in a window over Paris), and the
/// blend reaches [`BLEND_REACH`] pixels. A fractional value under the floor is
/// the evidence, and then every pixel within reach of a hole is a hole.
/// To make it fail: set `BLEND_REACH` to 1 (pixels at distance 3 survive), or
/// remove the `fract` test (nothing is a neighbour any more).
#[test]
fn a_resampled_answer_loses_the_pixels_its_blend_reached_and_no_more() {
    // Distance 1: -4000.5 (under the floor). Distance 2: -300.25 (over it, still
    // not a height). Distance 3 and 4: 50. The floor flags distance <= 1; the
    // reach of 2 pixels from those flags takes everything up to distance 3.
    let mut edits = Vec::new();
    for r in 2..=6usize {
        for c in 2..=6usize {
            let d = (r as i32 - 4).abs().max((c as i32 - 4).abs());
            match d {
                1 => edits.push(((r, c), -4000.5f32)),
                2 => edits.push(((r, c), -300.25f32)),
                _ => {}
            }
        }
    }
    let raster = read_geotiff(&field(-9999.0, &edits).build()).unwrap();
    let (hole, voids) = voids_of(&raster);
    assert_eq!(
        voids.undeclared, 9,
        "the centre and its eight neighbours are under the floor"
    );
    assert_eq!(
        voids.blended_ring, 40,
        "the 7 x 7 block is 49 pixels, 9 of them under the floor"
    );
    assert_eq!(voids.total(), 49);
    for r in 0..9usize {
        for c in 0..9usize {
            let d = (r as i32 - 4).abs().max((c as i32 - 4).abs());
            assert_eq!(hole[r * 9 + c], d <= 3, "pixel ({r}, {c})");
        }
    }
    let z = lattice_of(&raster).unwrap().z;
    assert!(
        z.iter().all(|v| v.is_nan() || *v == 50.0),
        "no blend survives as a height"
    );
}

/// Rule: a whole-number sentinel alone is no evidence of resampling, so a
/// nearest-neighbour answer keeps the valid pixels around its holes, and a real
/// depression (-400 m, the Dead Sea) is a height, not a hole.
/// To make it fail: dilate unconditionally, or raise `LOWEST_LAND_M` above -400.
#[test]
fn a_whole_sentinel_alone_costs_one_pixel_and_a_dead_sea_is_a_height() {
    let raster = read_geotiff(&field(-9999.0, &[((0, 0), -400.0)]).build()).unwrap();
    let (hole, voids) = voids_of(&raster);
    assert_eq!((voids.undeclared, voids.blended_ring), (1, 0));
    assert_eq!(hole.iter().filter(|h| **h).count(), 1);
    assert!(!hole[0], "-400 m is a height");
}

/// To make it fail: a lattice_of that accepts a number of values different from width x height.
#[test]
fn a_raster_whose_values_do_not_fit_its_shape_is_refused() {
    let mut raster = read_geotiff(&asymmetric().build()).unwrap();
    raster.values.pop();
    assert!(lattice_of(&raster).is_err());
}

/// To make it fail: accept a file that is compressed, big-endian, tiled, multiband, integer or without georeferencing.
#[test]
fn what_is_not_read_is_refused_by_name() {
    let refused = |tiff: &Tiff, word: &str| {
        let why = message(read_geotiff(&tiff.build()).unwrap_err());
        assert!(why.contains(word), "expected `{word}` in: {why}");
    };

    let mut t = asymmetric();
    t.compression = 8;
    refused(&t, "compression 8");

    let mut t = asymmetric();
    t.tiled = true;
    refused(&t, "tiles");

    let mut t = asymmetric();
    t.samples = 3;
    refused(&t, "3 bands");

    let mut t = asymmetric();
    t.bits = 16;
    t.sample_format = 1;
    refused(&t, "16-bit");

    let mut t = asymmetric();
    t.sample_format = 1; // 32-bit unsigned integers
    refused(&t, "SampleFormat 1");

    let mut t = asymmetric();
    t.georef = false;
    refused(&t, "georeferencing");

    let mut t = asymmetric();
    t.point_is_area = false;
    refused(&t, "PixelIsPoint");

    let mut big = asymmetric().build();
    big[0] = b'M';
    big[1] = b'M';
    assert!(message(read_geotiff(&big).unwrap_err()).contains("big-endian"));

    assert!(message(read_geotiff(b"hello, not a tiff").unwrap_err()).contains("not a TIFF"));
    assert!(message(read_geotiff(&[]).unwrap_err()).contains("truncated"));

    let mut cut = asymmetric().build();
    cut.truncate(cut.len() - 20);
    assert!(
        read_geotiff(&cut).is_err(),
        "a cut file is an error, not a short raster"
    );
}

// -- asking for a window, and reading what comes back ------------------------

/// Paris, in Lambert-93 and in degrees — covered by every IGN relief source.
const PARIS: [f64; 4] = [651_000.0, 6_860_000.0, 652_000.0, 6_861_000.0];
const PARIS_WGS84: [f64; 4] = [2.33, 48.85, 2.35, 48.87];

/// **The window grows to whole pixels from its south-west corner**, and says
/// it does. To make it fail: round `ceil` to `floor` — 1,000 m at 300 m would give
/// 3 pixels and 900 m, a 100 m band lost without saying so.
#[test]
fn a_window_is_snapped_to_whole_pixels_of_the_step() {
    let catalogue = builtin();
    let ask = terrain::ask(&catalogue, Some("ign-rge-alti"), PARIS, 2154, PARIS_WGS84, 300.0).expect("asked");
    assert_eq!((ask.width, ask.height), (4, 4));
    assert_eq!(ask.bbox, [651_000.0, 6_860_000.0, 652_200.0, 6_861_200.0]);
    assert_eq!(ask.layer(), "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES");
    assert_eq!(ask.service(), "https://data.geopf.fr/wms-r");
    let query = query_of(&ask.request.url);
    assert_eq!(
        numbers(&query["BBOX"]),
        vec![651_000.0, 6_860_000.0, 652_200.0, 6_861_200.0]
    );
    assert_eq!((query["WIDTH"].as_str(), query["HEIGHT"].as_str()), ("4", "4"));
}

/// **Left to choose, it takes the finest relief source it can read** — never
/// one whose protocol is not read yet. To make it fail: remove the filter on
/// `WmsRaster` — Copernicus or the terrarium tiles, coarser here, would not
/// win, but a fine COG source would win and then refuse.
#[test]
fn left_to_choose_the_finest_readable_relief_source_is_taken() {
    let catalogue = builtin();
    let ask = terrain::ask(&catalogue, None, PARIS, 2154, PARIS_WGS84, 50.0).expect("asked");
    assert!(
        matches!(ask.source.protocol, terrain::Protocol::WmsRaster { .. }),
        "{}",
        ask.source.id
    );
    let readable: Vec<f64> = catalogue
        .covering(terrain::SourceKind::Relief, PARIS_WGS84)
        .into_iter()
        .filter(|one| matches!(one.protocol, terrain::Protocol::WmsRaster { .. }))
        .map(|one| one.resolution_m)
        .collect();
    assert_eq!(
        ask.source.resolution_m,
        readable.iter().cloned().fold(f64::MAX, f64::min)
    );
}

/// Refused by name, each with what to do. To make it fail: let an image through
/// as relief, or a window that is too wide without saying the step that would fit.
#[test]
fn a_window_is_refused_by_name_before_anything_is_asked() {
    let catalogue = builtin();
    let too_wide =
        message(terrain::ask(&catalogue, Some("ign-rge-alti"), PARIS, 2154, PARIS_WGS84, 0.1).unwrap_err());
    assert!(
        too_wide.contains("5010") && too_wide.contains("a step of 1 or more"),
        "{too_wide}"
    );
    let unknown =
        message(terrain::ask(&catalogue, Some("nowhere"), PARIS, 2154, PARIS_WGS84, 50.0).unwrap_err());
    assert!(
        unknown.contains("no source `nowhere`") && unknown.contains("ign-rge-alti"),
        "{unknown}"
    );
    let imagery =
        message(terrain::ask(&catalogue, Some("ign-ortho"), PARIS, 2154, PARIS_WGS84, 50.0).unwrap_err());
    assert!(imagery.contains("a terrain is relief"), "{imagery}");
    let flat =
        message(terrain::ask(&catalogue, Some("ign-rge-alti"), PARIS, 2154, PARIS_WGS84, 0.0).unwrap_err());
    assert!(flat.contains("positive"), "{flat}");
    let far = [-80.0, -40.0, -79.9, -39.9];
    let outside =
        message(terrain::ask(&catalogue, Some("ign-lidar-hd-mnt"), PARIS, 2154, far, 50.0).unwrap_err());
    assert!(outside.contains("does not cover"), "{outside}");
}

fn answer(status: u16, body: Vec<u8>) -> energyml_providers::transport::Answer {
    energyml_providers::transport::Answer {
        status,
        content_type: None,
        body: Box::new(std::io::Cursor::new(body)),
    }
}

/// **A refusal is the server's words**, even sent with a 200: a WMS answers
/// a `ServiceException` in XML. To make it fail: read every body as a TIFF — we
/// would say *not a TIFF* instead of what the service explained.
#[test]
fn what_a_service_answers_instead_of_a_tiff_is_its_own_refusal() {
    let xml = b"<ServiceExceptionReport><ServiceException code=\"InvalidCRS\">CRS EPSG:9999 unknown</ServiceException></ServiceExceptionReport>".to_vec();
    let refused = terrain::read_answer(answer(200, xml), 1 << 20).unwrap_err();
    assert!(
        matches!(&refused, Error::Server { status: 200, .. }),
        "{refused:?}"
    );
    assert!(message(refused).contains("EPSG:9999 unknown"));
    let tiff = asymmetric().build();
    let raster = terrain::read_answer(answer(200, tiff), 1 << 20).expect("a tiff reads");
    assert_eq!((raster.width, raster.height), (3, 2));
    let small = terrain::read_answer(answer(200, asymmetric().build()), 10).unwrap_err();
    assert!(matches!(small, Error::TooMuch(_)), "{small:?}");
}

// ---------------------------------------------------------------------------
// What a face needs besides the request.

/// To make it fail: reproject only two corners (a box that the projection
/// rotates would be cut), or return the envelope out of order.
#[test]
fn the_area_in_degrees_envelopes_all_four_corners() {
    // A fake projection that shears: x' = x + y/10, y' = y. Two opposite corners
    // alone would miss the east edge of the top row.
    let shear =
        |p: [f64; 3]| -> std::result::Result<[f64; 3], String> { Ok([p[0] + p[1] / 10.0, p[1], 0.0]) };
    let degrees = terrain::area_wgs84([0.0, 0.0, 10.0, 20.0], 2154, shear).unwrap();
    assert_eq!(degrees, [0.0, 0.0, 12.0, 20.0]);
    // Degrees already: untouched, the projection never asked.
    let never = |_: [f64; 3]| -> std::result::Result<[f64; 3], String> { Err("asked".into()) };
    assert_eq!(
        terrain::area_wgs84([1.0, 2.0, 3.0, 4.0], 4326, never).unwrap(),
        [1.0, 2.0, 3.0, 4.0]
    );
    // A failing projection and a degenerate area are refused by name.
    assert!(
        terrain::area_wgs84([0.0, 0.0, 1.0, 1.0], 2154, never)
            .unwrap_err()
            .to_string()
            .contains("asked")
    );
    assert!(terrain::area_wgs84([1.0, 0.0, 0.0, 1.0], 2154, shear).is_err());
}

/// To make it fail: swap `TILEROW` and `TILECOL` — the image lands in the wrong
/// place and stays plausible —, or give a template to another tile set.
#[test]
fn a_wmts_in_pm_is_a_zxy_template_and_nothing_else_is() {
    let catalogue = TerrainCatalogue::parse(
        r#"{"sources":[
      {"id":"pm","title":"t","kind":"imagery","protocol":{"type":"wmts","url":"https://x.test/wmts","layer":"A.B","tileMatrixSet":"PM","format":"image/jpeg","style":"normal"},
       "coverage":{"boxes":[[0,0,1,1]]},"crs":[3857],"resolutionM":1,"licence":"l","attribution":"a","needsKey":false},
      {"id":"nostyle","title":"t","kind":"imagery","protocol":{"type":"wmts","url":"https://x.test/wmts","layer":"A","tileMatrixSet":"PM","format":"image/png"},
       "coverage":{"boxes":[[0,0,1,1]]},"crs":[3857],"resolutionM":1,"licence":"l","attribution":"a","needsKey":false},
      {"id":"geo","title":"t","kind":"imagery","protocol":{"type":"wmts","url":"https://x.test/wmts","layer":"A","tileMatrixSet":"WGS84","format":"image/png"},
       "coverage":{"boxes":[[0,0,1,1]]},"crs":[4326],"resolutionM":1,"licence":"l","attribution":"a","needsKey":false},
      {"id":"wms","title":"t","kind":"relief","protocol":{"type":"wmsRaster","url":"https://x.test/wms","layer":"A","format":"image/geotiff"},
       "coverage":{"boxes":[[0,0,1,1]]},"crs":[2154],"resolutionM":1,"licence":"l","attribution":"a","needsKey":false}
    ]}"#,
    )
    .unwrap();
    let template = terrain::tile_template(catalogue.get("pm").unwrap()).unwrap();
    assert_eq!(
        template,
        "https://x.test/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=A.B&STYLE=normal&TILEMATRIXSET=PM&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}&FORMAT=image%2Fjpeg"
    );
    // No style declared: the standard's `default`.
    assert!(
        terrain::tile_template(catalogue.get("nostyle").unwrap())
            .unwrap()
            .contains("STYLE=default")
    );
    assert!(
        terrain::tile_template(catalogue.get("geo").unwrap())
            .unwrap_err()
            .to_string()
            .contains("WGS84")
    );
    assert!(
        terrain::tile_template(catalogue.get("wms").unwrap())
            .unwrap_err()
            .to_string()
            .contains("not a WMTS")
    );
}
