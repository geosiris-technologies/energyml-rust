//! Reading a foreign mesh — OBJ and legacy VTK — in the currency of a shape.
//!
//! # Why here
//!
//! Inputs must also be able to import mesh files (not only GeoJSON), and their
//! reading goes through Rust to avoid duplicating the reading in the front end.
//!
//! The supposition that the Rust part surely exposes the formats it supports did
//! not hold, and it was worth checking: **this repository only ever wrote these
//! formats**. `energyml-export` is their writer; the reader lived in the front end —
//! an OBJ parser and a VTK parser in JavaScript. Two
//! implementations of one format would have ended up accepting different things
//! without anything saying so, and the front end is the side that neither `cargo
//! test` nor the CLI reaches.
//!
//! # Where it lives
//!
//! Next to [`crate::geojson`], and for the same reason that one gives: *a parser
//! and a vocabulary, no storage, no I/O*. What can be wrong is here, testable
//! without a file; the layer that opens a package knows nothing of it.
//!
//! # What it returns
//!
//! A [`Geometry`], the currency of the repository — what the readers produce and
//! what the exporters consume. Not a structure of our own: the caller puts it
//! in a bag, and the fifteen operations read it like any other shape.

use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// A format this module can **read**.
///
/// **The list is here, and it is the only one**: a caller asks what is
/// readable rather than spelling extensions on its own side. A format added
/// appears in a front end's file picker without a line changing there, which is
/// this repository's rule for everything that grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    /// Its short name, which is also the recipe's: `obj`, `vtk`.
    pub name: &'static str,
    /// The extensions a file picker offers, the dot included.
    pub extensions: &'static [&'static str],
    /// What is read of it, in one line and in English.
    pub describe: &'static str,
    /// What this format does not say and has to be given.
    ///
    /// **The list is here for the same reason as the extensions**: a front end that
    /// knew that "the CSV asks for a height" would be spelling a format on its
    /// own side, and the next time the list changes there would be two
    /// truths. It asks the module what is missing, and renders one field per
    /// entry.
    ///
    /// Empty for most: an OBJ carries its three coordinates and a system
    /// nobody asks of it.
    pub asks: &'static [Ask],
}

/// A value a format requires from the caller, because the file is silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// At which height to place points that have none.
    ///
    /// A two-column cloud is a map, not a shape: placing it at zero would be
    /// choosing an elevation on someone's behalf, and zero is no more neutral
    /// than any other — it is sea level in half the systems and the bottom of
    /// the model in the other half.
    Height,
    /// In which projected system to place degrees.
    ///
    /// A GeoJSON is in WGS 84 by definition (RFC 7946) and a scene is in
    /// metres. Without reprojection, an extent of 0.25° falls beside a model
    /// eighteen kilometres wide and a thousand times too small — the exact
    /// defect a SIGEOL import once cost. The code is not guessed:
    /// `crs::utm_epsg_of` **proposes** one, drawn from the coordinates.
    Projection,
}

impl Ask {
    /// Its name, for a front end that renders one field per entry.
    pub fn name(self) -> &'static str {
        match self {
            Ask::Height => "height",
            Ask::Projection => "projection",
        }
    }
}

/// What a refusal says, and **what should have been given**.
///
/// # Why the list, and not only the sentence
///
/// [`Format::asks`] says what a format *may* require; it does not say what
/// *this* file does not carry. A BRGM GeoJSON writes
/// `[2.2953, 48.875, 58]` — third ordinate included — and its UTM zone is
/// deduced from its longitude: it needs nothing. Asking anyway is a
/// question the caller has already answered without knowing it.
///
/// Hence the order: **we try, and we only ask if the refusal is for that.** A
/// single pass in the ordinary case, and a form that opens only
/// when it has something to ask.
///
/// # Why the refusal carries it
///
/// The first version recognised the lack by words in the message —
/// `why.contains("give a height")`. Two truths for one fact, in two
/// places: the day the sentence changes, the question stops being asked and
/// **nothing says so**. The form no longer opens, and the caller concludes that
/// the file is unreadable. The list is therefore built where the refusal is
/// born, and the compiler holds the two together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// What is shown: a sentence that names the file and its line.
    pub why: String,
    /// What was missing — **empty when no answer would lift this refusal**. A
    /// binary VTK is not repaired by filling in a field, and opening a form
    /// for it would promise what cannot be kept.
    pub missing: Vec<Ask>,
}

impl Refused {
    /// A refusal that no answer will lift.
    pub fn flat(why: impl Into<String>) -> Self {
        Refused { why: why.into(), missing: Vec::new() }
    }

    /// A refusal that names what it expects.
    pub fn wanting(why: impl Into<String>, missing: Vec<Ask>) -> Self {
        Refused { why: why.into(), missing }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

/// Where a reading placed the degrees a format carries — and **who** decided it.
///
/// # Why the reading says so
///
/// A report from a user: the GeoJSON import showed nothing in the scene unless
/// EPSG codes to reproject onto were given by hand — is that normal?
///
/// It was not normal, and half of the cause was here. With no code given,
/// [`read_geojson`] carried the degrees into the UTM zone of the first position
/// — a **proposal** drawn from the numbers, which this module applied without
/// saying so. In a Lambert-93 scene, a GeoJSON of the Paris basin placed in
/// UTM 31N arrives some 1 450 km to the south-west of the model: read, drawn,
/// and out of sight. And the caller could say nothing about it, since nothing
/// told it in which system the numbers had arrived.
///
/// `Placed` says it, and says whether the code was **given or proposed**: a
/// proposal applied silently is a hidden defect; said, it is a
/// suggestion that can be replaced — the rule that `crs::utm_epsg_of` already
/// writes for SIGEOL imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// The EPSG code of the coordinates returned.
    pub epsg: i64,
    /// Chosen here — the UTM zone of the first position — rather than given.
    pub proposed: bool,
    /// This code measures **degrees**: the positions stayed longitudes and
    /// latitudes. Placed in a scene in metres, they fall thousands of
    /// kilometres from everything in it — the case a caller must be able to
    /// name rather than draw.
    pub degrees: bool,
}

/// A shape read, and where its degrees were placed.
///
/// `placed` is `None` for a format that carries no degrees — an OBJ, a
/// VTK, a CSV are in the scene's system or in none, and this module has
/// decided nothing in their place. It is an absence, not a "nothing moved".
#[derive(Debug, Clone)]
pub struct Read {
    /// The shape, as [`read_or_ask`] returns it.
    pub geometry: Geometry,
    /// Where the degrees went, when the format carried any.
    pub placed: Option<Placed>,
}

/// What the caller supplies to a format that asks for it.
///
/// A field named by `Ask` rather than a bag of keys: two possible values,
/// two fields, and the compiler says which one is missing. A bag would only say
/// so at run time, and only the day somebody opens that format.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Given {
    /// The elevation at which to place a cloud that carries none.
    pub height: Option<f64>,
    /// The projected EPSG code in which to carry degrees.
    pub projection: Option<i64>,
}

/// Everything this module reads, in the order a picker offers them.
pub const FORMATS: &[Format] = &[
    Format {
        name: "obj",
        extensions: &[".obj"],
        describe: "Wavefront OBJ: vertices, faces fanned into triangles, `l` chains as segments",
        asks: &[],
    },
    Format {
        name: "vtk",
        extensions: &[".vtk"],
        describe: "legacy ASCII VTK: POLYDATA and UNSTRUCTURED_GRID, cells prefixed by their count",
        asks: &[],
    },
    Format {
        name: "vtp",
        extensions: &[".vtp"],
        describe: "VTK XML PolyData (.vtp), ascii DataArrays: points, polys, lines, strips",
        asks: &[],
    },
    Format {
        name: "vtu",
        extensions: &[".vtu"],
        describe: "VTK XML UnstructuredGrid (.vtu), ascii DataArrays: points, connectivity, offsets, types",
        asks: &[],
    },
    Format {
        name: "csv",
        extensions: &[".csv"],
        describe: "a point cloud: one row per point, x/y/z by header or by position",
        asks: &[Ask::Height],
    },
    Format {
        name: "geojson",
        extensions: &[".geojson", ".json"],
        describe: "RFC 7946 GeoJSON in WGS 84: places, lines and rings, projected into a metric system",
        asks: &[Ask::Projection, Ask::Height],
    },
];

/// VTK XML — `.vtp` and `.vtu` — whose `DataArray`s are `ascii`.
///
/// # One reader for both
///
/// They differ only by the names of their sections: a `.vtp` keeps its
/// cells under `Polys`, `Lines` and `Strips`, a `.vtu` under `Cells`. The rest
/// is identical — `Points`, then `connectivity`/`offsets` that say where
/// each cell ends. Two readers would have been the same thing twice,
/// with an even chance that a fix goes into only one of them.
///
/// # What it refuses, and it names it
///
/// `format="binary"` and `format="appended"` exist, and we write neither.
/// Reading them requires base64, zlib compression and the offsets of the
/// *appended* blocks — three things that would be done by halves. A refusal that
/// names the encoding says what to do (re-export as ascii); a reader that
/// returned an empty mesh would say the file is empty.
///
/// # `offsets` and not counts
///
/// VTK XML gives the **end** of each cell, not its length: cell `i`
/// runs from `offsets[i-1]` to `offsets[i]`. Reading the array as lengths
/// gives a mesh that is plausible and wrong — the first cells are right,
/// the next ones drift — which no assertion on a number of points would
/// see.
pub fn read_vtk_xml(text: &str) -> Result<Geometry, String> {
    if let Some(bad) = ["\"binary\"", "\"appended\""].iter().find(|kind| text.contains(**kind)) {
        return Err(format!(
            "this file has {} DataArrays; this build reads `ascii` only — re-export it as ascii",
            bad.trim_matches('"')
        ));
    }
    let points = xml_array(text, Some("Points"), None)
        .ok_or_else(|| "no <Points> section with an ascii DataArray".to_string())?;
    if points.len() % 3 != 0 {
        return Err(format!("{} coordinate(s) is not a whole number of points", points.len()));
    }
    let places: Vec<[f64; 3]> = points
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();

    let mut triangles = Vec::new();
    let mut segments = Vec::new();
    for (section, as_area) in [("Polys", true), ("Strips", true), ("Cells", true), ("Lines", false)] {
        let Some(connectivity) = xml_array(text, Some(section), Some("connectivity")) else {
            continue;
        };
        let offsets = xml_array(text, Some(section), Some("offsets")).unwrap_or_default();
        // **A `.vtu` says what each cell is in `types`**, and the points of a
        // cell mean nothing without it: the eight points of a hexahedron fanned
        // from the first corner are diagonals through the cell, not its faces.
        // Only `Cells` carries the array; `Polys`, `Lines` and `Strips` are
        // polygons, polylines and strips by their section name.
        let types = if section == "Cells" { xml_array(text, Some(section), Some("types")) } else { None };
        // VTK_POLYHEDRON keeps its faces apart from the connectivity:
        // `faceoffsets[i]` is where cell `i`'s run in `faces` ends.
        let faces = xml_array(text, Some(section), Some("faces")).unwrap_or_default();
        let face_ends = xml_array(text, Some(section), Some("faceoffsets")).unwrap_or_default();
        if types.as_ref().is_some_and(|kinds| kinds.len() != offsets.len()) {
            return Err(format!("<{section}> has {} offsets but {} types", offsets.len(), types.map_or(0, |k| k.len())));
        }
        let mut from = 0usize;
        let mut face_from = 0usize;
        for (at, end) in offsets.iter().map(|v| *v as usize).enumerate() {
            if end > connectivity.len() || end < from {
                return Err(format!("<{section}> offsets run past its connectivity"));
            }
            let cell: Vec<u32> = connectivity[from..end].iter().map(|v| *v as u32).collect();
            from = end;
            if cell.iter().any(|i| *i as usize >= places.len()) {
                return Err(format!("<{section}> names a point this file does not have"));
            }
            if section == "Strips" {
                // A strip is not a fan: triangle `k` is `k, k+1, k+2`, with every
                // other one reversed so they all face the same way.
                for k in 0..cell.len().saturating_sub(2) {
                    if k % 2 == 0 {
                        triangles.push([cell[k], cell[k + 1], cell[k + 2]]);
                    } else {
                        triangles.push([cell[k + 1], cell[k], cell[k + 2]]);
                    }
                }
            } else if let Some(kinds) = &types {
                let kind = kinds[at] as i64;
                if kind == 42 {
                    let stop = face_ends.get(at).map(|v| *v as usize).filter(|stop| *stop <= faces.len() && *stop >= face_from);
                    let Some(stop) = stop else {
                        return Err(format!("<{section}> polyhedron {at} has no usable `faces`/`faceoffsets`"));
                    };
                    let run: Vec<u32> = faces[face_from..stop].iter().map(|v| *v as u32).collect();
                    face_from = stop;
                    polyhedron_faces(&run, places.len(), &mut triangles)?;
                } else {
                    cell_triangles(kind, &cell, &mut triangles, &mut segments)?;
                }
            } else if as_area && cell.len() >= 3 {
                fan(&cell, &mut triangles);
            } else if cell.len() >= 2 {
                for step in 1..cell.len() {
                    segments.push([cell[step - 1], cell[step]]);
                }
            }
        }
    }
    if places.is_empty() {
        return Err("no point in this file".to_string());
    }
    Ok(one_patch("vtk-xml", places, triangles, segments))
}

/// The numbers of a `<DataArray>`, looked up under a section and by its `Name`.
///
/// A text scan and not an XML tree: what is sought is a run of
/// numbers between two known tags, and building a tree to find it
/// would ask the crate for a dependency that its wasm target would pay on every
/// page — to read six files. The refusal above covers what this scan
/// could not read.
fn xml_array(text: &str, section: Option<&str>, named: Option<&str>) -> Option<Vec<f64>> {
    let hay = match section {
        Some(name) => {
            let open = text.find(&format!("<{name}"))?;
            let close = text[open..].find(&format!("</{name}>"))? + open;
            &text[open..close]
        }
        None => text,
    };
    let mut at = 0usize;
    while let Some(start) = hay[at..].find("<DataArray") {
        let head_at = at + start;
        let head_end = hay[head_at..].find('>')? + head_at;
        let head = &hay[head_at..head_end];
        let body_end = hay[head_end..].find("</DataArray>")? + head_end;
        at = body_end + 1;
        if let Some(wanted) = named {
            if !head.contains(&format!("Name=\"{wanted}\"")) {
                continue;
            }
        }
        return Some(
            hay[head_end + 1..body_end]
                .split_ascii_whitespace()
                .filter_map(|word| word.parse::<f64>().ok())
                .collect(),
        );
    }
    None
}

/// A cloud of points, one row per point.
///
/// # What it accepts, and why so wide
///
/// A CSV has no standard: the separator is a comma, a semicolon
/// or a tab depending on the spreadsheet that wrote it, and the header may be missing.
/// Refusing whatever is not exactly `x,y,z` would refuse most of the
/// files we receive — so the columns are **named if they are**
/// (`x`/`y`/`z`, `easting`/`northing`, `lon`/`lat`… no: see below) and
/// taken by position otherwise.
///
/// What is *not* guessed, on the other hand: **degrees**. A column named
/// `lon` would be a GeoJSON in disguise, and reading it as metres would place the
/// cloud beside the model and a thousand times too small. Geographic headers
/// are therefore refused by name, with the format that can read them.
///
/// # The missing height
///
/// Two columns make a map, not a shape. Placing the cloud at zero would be
/// choosing an elevation on someone's behalf — and zero is not neutral: it is
/// sea level in half the vertical systems and the bottom of the model
/// in the other half. The format **asks** for it (`Ask::Height`), and without an answer it
/// refuses, saying what to give.
pub fn read_csv(text: &str, height: Option<f64>) -> Result<Geometry, Refused> {
    let mut points = Vec::new();
    let mut columns: Option<(usize, usize, Option<usize>)> = None;
    for (at, row) in text.lines().enumerate() {
        let row = row.trim_start_matches('\u{feff}').trim();
        if row.is_empty() || row.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = split_row(row);
        if columns.is_none() {
            columns = Some(match header(&cells) {
                Some(found) => found.map_err(Refused::flat)?,
                // No header: the first two or three columns, in order. That is
                // what any export without titles writes, and the only order a
                // nameless cloud can have.
                None => (0, 1, (cells.len() > 2).then_some(2)),
            });
            // A header row carries no point.
            if header(&cells).is_some() {
                continue;
            }
        }
        let (ix, iy, iz) = columns.expect("set just above");
        // `column` and not `at`: the latter is the **row** number, and a closure
        // that shadowed it would name the column in a message that announces a
        // row. A refusal that points to the wrong place sends one looking where
        // there is nothing, which is worse than no number.
        let number = |column: usize| -> Result<f64, Refused> {
            cells
                .get(column)
                .map(|cell| cell.trim().replace(',', "."))
                .ok_or_else(|| {
                    Refused::flat(format!(
                        "row {}: column {} asked for, only {} there",
                        at + 1,
                        column + 1,
                        cells.len()
                    ))
                })?
                .parse::<f64>()
                .map_err(|_| {
                    Refused::flat(format!(
                        "row {}, column {}: `{}` is not a number",
                        at + 1,
                        column + 1,
                        cells.get(column).unwrap_or(&"")
                    ))
                })
        };
        let z = match iz {
            Some(column) => number(column)?,
            // **The refusal names what it expects**, here and not in the caller:
            // recognising the lack by words in the message would be two truths
            // for one fact, and the day the sentence changes, the question
            // stops being asked without anything saying so.
            None => height.ok_or_else(|| {
                Refused::wanting(
                    format!(
                        "row {}: two columns and no height — this file states where, not how deep",
                        at + 1
                    ),
                    vec![Ask::Height],
                )
            })?,
        };
        points.push([number(ix)?, number(iy)?, z]);
    }
    if points.is_empty() {
        return Err(Refused::flat(
            "no point in this file: every row was empty, a comment, or a header",
        ));
    }
    Ok(one_patch("csv", points, Vec::new(), Vec::new()))
}

/// The separator is the one that splits the most — measured, never assumed.
fn split_row(row: &str) -> Vec<&str> {
    [';', '\t', ',']
        .iter()
        .map(|sep| row.split(*sep).collect::<Vec<_>>())
        .max_by_key(Vec::len)
        .unwrap_or_default()
}

/// The columns a header row names, or `None` if it is not one.
///
/// Returns a refusal when the header is geographic: better to name the format
/// that can read it than to put degrees in a metric scene.
#[allow(clippy::type_complexity)]
fn header(cells: &[&str]) -> Option<Result<(usize, usize, Option<usize>), String>> {
    let named: Vec<String> = cells.iter().map(|c| c.trim().to_ascii_lowercase()).collect();
    let find = |wanted: &[&str]| named.iter().position(|c| wanted.contains(&c.as_str()));
    if find(&["lon", "longitude", "lng"]).is_some() && find(&["lat", "latitude"]).is_some() {
        return Some(Err(
            "this file's columns are longitude and latitude: a scene is in metres, and degrees \
placed in one land beside the model and a thousand times too small — write it as GeoJSON, which \
this build projects, or give the columns projected coordinates"
                .to_string(),
        ));
    }
    let x = find(&["x", "easting", "e"])?;
    let y = find(&["y", "northing", "n"])?;
    Some(Ok((x, y, find(&["z", "elevation", "depth", "alt", "altitude"]))))
}

/// A GeoJSON collection, carried into a metric system.
///
/// # Why it requires an EPSG code
///
/// RFC 7946 fixes the system: WGS 84, in degrees. A scene is in metres. Without
/// reprojection, an extent of 0.25° is placed beside a model eighteen
/// kilometres wide and a thousand times too small — the exact defect a SIGEOL
/// import once cost, and it passes `check` without a word.
///
/// The code is not guessed for all that: a projection moves every
/// coordinate, so it is a decision. `crs::utm_epsg_of` **proposes** one,
/// drawn from the longitude of the data itself, and the caller replaces it —
/// provided it knows it was proposed: see [`read_geojson_placed`].
pub fn read_geojson(
    text: &str,
    projection: Option<i64>,
    height: Option<f64>,
) -> Result<Geometry, Refused> {
    read_geojson_placed(text, projection, height).map(|read| read.geometry)
}

/// The same reading, and **the system in which it placed the degrees** — see
/// [`Placed`].
///
/// A scene that already has a system must receive the file in it: any
/// other code returns numbers that are right in their system and wrong in its
/// own, that is, an object drawn elsewhere. This module does not know the scene;
/// what it can do is say what it did, so that whoever knows the scene
/// can compare.
pub fn read_geojson_placed(
    text: &str,
    projection: Option<i64>,
    height: Option<f64>,
) -> Result<Read, Refused> {
    let read = crate::geojson::read(text).map_err(|why| Refused::flat(why.0))?;
    let first = read
        .features
        .iter()
        .flat_map(|feature| feature.positions())
        .next()
        .ok_or_else(|| Refused::flat("no position in this collection"))?;
    let epsg = projection
        .or_else(|| crate::crs::utm_epsg_of(first.lon, first.lat))
        .ok_or_else(|| {
            Refused::wanting(
                "this collection's coordinates name no UTM zone, so no metric system can be \
proposed from them",
                vec![Ask::Projection],
            )
        })?;
    let target = crate::crs::Target::Projected(epsg);

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();
    for feature in &read.features {
        for part in &feature.parts {
            let placed = crate::crs::place(part, &target, 4326).map_err(Refused::flat)?;
            let first = points.len() as u32;
            // **A position without a third ordinate comes back as `NaN`**, and a
            // `NaN` is not seen: it draws nothing, and it poisons every
            // bounding box that meets it. RFC 7946 makes the altitude
            // optional, so this is the ordinary case and not the edge case.
            // The requested height replaces it; without one, we refuse, saying
            // so, rather than placing everything at zero.
            for point in &placed {
                let z = if point[2].is_finite() {
                    point[2]
                } else {
                    height.ok_or_else(|| {
                        Refused::wanting(
                            "this collection's positions carry no elevation, which RFC 7946 \
makes optional",
                            vec![Ask::Height],
                        )
                    })?
                };
                points.push([point[0], point[1], z]);
            }
            // **A line, not a surface.** A GeoJSON ring is closed by
            // repeating its first point, so joining its vertices two by
            // two closes it by itself; triangulating it would be deciding it is
            // filled, which the format does not say.
            for step in 1..placed.len() as u32 {
                segments.push([first + step - 1, first + step]);
            }
        }
    }
    if points.is_empty() {
        return Err(Refused::flat("no position in this collection"));
    }
    Ok(Read {
        geometry: one_patch("geojson", points, Vec::new(), segments),
        placed: Some(Placed {
            epsg,
            proposed: projection.is_none(),
            // *I do not know* is not *no* (`crs::is_geographic`): a code
            // unknown to this build does not get here — `place` would have
            // refused it, naming it — except 4326 onto itself, which is known.
            degrees: crate::crs::is_geographic(epsg) == Some(true),
        }),
    })
}

/// The format of a file name, or `None` if none claims it.
///
/// On the **extension**, case-insensitive. Sniffing the content would be a second
/// way of deciding the format, and the two would end up no longer saying the same
/// thing of the same file — each reader already refuses by name what is not
/// of its dialect.
pub fn format_of(name: &str) -> Option<&'static Format> {
    let lower = name.to_ascii_lowercase();
    FORMATS.iter().find(|format| format.extensions.iter().any(|ext| lower.ends_with(ext)))
}

/// Read a mesh, the format decided by the file name.
///
/// The refusal **names both ends**: the file and what we can read.
pub fn read(name: &str, text: &str) -> Result<Geometry, String> {
    read_given(name, text, &Given::default())
}

/// The same reading, with what the caller was able to supply.
///
/// Separate from [`read`] rather than adding an argument to it: most
/// formats ask for nothing, and callers with nothing to give need not
/// write that they give nothing.
pub fn read_given(name: &str, text: &str, given: &Given) -> Result<Geometry, String> {
    read_or_ask(name, text, given).map_err(|refused| refused.why)
}

/// The same reading, but its refusal says **what was missing** — see [`Refused`].
pub fn read_or_ask(name: &str, text: &str, given: &Given) -> Result<Geometry, Refused> {
    read_placed(name, text, given).map(|read| read.geometry)
}

/// The same reading, and **where it placed the degrees** of the file — see
/// [`Placed`]. What a scene calls, because only it can compare that
/// system with its own.
pub fn read_placed(name: &str, text: &str, given: &Given) -> Result<Read, Refused> {
    // Nothing was decided for these formats: they are in the scene's system
    // or in none, and saying `None` rather than a code prevents a
    // caller from comparing a system that nobody chose.
    let unplaced = |geometry: Geometry| Read { geometry, placed: None };
    match format_of(name) {
        Some(format) if format.name == "obj" => read_obj(text).map(unplaced).map_err(Refused::flat),
        Some(format) if format.name == "vtk" => read_vtk(text).map(unplaced).map_err(Refused::flat),
        Some(format) if format.name == "vtp" || format.name == "vtu" => {
            read_vtk_xml(text).map(unplaced).map_err(Refused::flat)
        }
        // The only two that can lack something — and they only
        // find out by reading: a `z` column may be there, an altitude
        // may be written position by position.
        Some(format) if format.name == "csv" => read_csv(text, given.height).map(unplaced),
        Some(format) if format.name == "geojson" => {
            read_geojson_placed(text, given.projection, given.height)
        }
        _ => Err(Refused::flat(format!(
            "`{name}` is not a mesh this build reads — it reads {}",
            FORMATS.iter().map(|f| f.name).collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// A single-patch shape, the way a foreign file arrives.
fn one_patch(kind: &'static str, points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>, segments: Vec<[u32; 2]>) -> Geometry {
    // **Triangles win over segments when there are both.** A patch carries one
    // topology; an OBJ that declares faces *and* `l`s is a surface with lines
    // on it, and it is the surface that describes it. The segments then
    // become a second patch, on the same points — which `node_offset` can
    // express.
    let mut patches = Vec::new();
    if !triangles.is_empty() {
        patches.push(Patch {
            index: 0,
            node_offset: 0,
            points: points.clone(),
            topology: Topology::Triangles(triangles),
            closed: Vec::new(),
        });
    }
    if !segments.is_empty() {
        // **The offset is the running sum**, which `Patch::node_offset`
        // defines — and not zero because the two patches carry the same
        // points. A zero here and everything that indexes *the representation's
        // nodes* would designate the nodes of the first patch believing it
        // named those of the second: two readers agreeing on a total and
        // disagreeing on what it means.
        let node_offset = patches.iter().map(|patch: &Patch| patch.points.len()).sum();
        patches.push(Patch {
            index: patches.len(),
            node_offset,
            points: points.clone(),
            topology: Topology::Segments(segments),
            closed: Vec::new(),
        });
    }
    if patches.is_empty() {
        patches.push(Patch { index: 0, node_offset: 0, points, topology: Topology::Points, closed: Vec::new() });
    }
    Geometry { kind, patches }
}

/// Read an OBJ.
///
/// Covers what real exports write: `v x y z` (extra components — vertex
/// colours, `w` — are ignored), `f` in the three spellings of
/// indices (`7`, `7/2`, `7//3`), **negative** indices (relative to the end,
/// as the format defines), polygons fanned into triangles, and `l` as
/// chains of segments. Everything else — `vt`, `vn`, `usemtl`, comments —
/// passes silently: it is real data, not an error.
///
/// Refuses **naming the line**: a zero index (the format counts from
/// 1), an index outside the buffer, a file without any vertex.
pub fn read_obj(text: &str) -> Result<Geometry, String> {
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();

    for (at, row) in text.lines().enumerate() {
        let line = at + 1;
        let mut words = row.split_whitespace();
        match words.next() {
            Some("v") => {
                let numbers: Vec<f64> = words.take(3).filter_map(|w| w.parse().ok()).collect();
                if numbers.len() < 3 {
                    return Err(format!("line {line}: `v` needs three numbers"));
                }
                points.push([numbers[0], numbers[1], numbers[2]]);
            }
            Some("f") => {
                let ring = indices(words, points.len(), line)?;
                // The fan from the first corner, which is right for the
                // convex rings an OBJ writes.
                for step in 1..ring.len().saturating_sub(1) {
                    triangles.push([ring[0], ring[step], ring[step + 1]]);
                }
            }
            Some("l") => {
                let chain = indices(words, points.len(), line)?;
                for pair in chain.windows(2) {
                    segments.push([pair[0], pair[1]]);
                }
            }
            _ => {}
        }
    }

    if points.is_empty() {
        return Err("no vertex (`v`) in this file — it is not an OBJ, or it is empty".to_string());
    }
    Ok(one_patch("obj", points, triangles, segments))
}

/// The indices of a face or a line, brought back to base 0.
fn indices<'a>(words: impl Iterator<Item = &'a str>, count: usize, line: usize) -> Result<Vec<u32>, String> {
    let mut out = Vec::new();
    for word in words {
        // `7/2` and `7//3`: only the first field is the vertex.
        let first = word.split('/').next().unwrap_or("");
        if first.is_empty() {
            continue;
        }
        let Ok(index) = first.parse::<i64>() else {
            return Err(format!("line {line}: `{word}` is not an index"));
        };
        if index == 0 {
            return Err(format!("line {line}: index 0 does not exist — the format counts from 1"));
        }
        // Negative: relative to the end, as the format defines.
        let at = if index < 0 { count as i64 + index } else { index - 1 };
        if at < 0 || at >= count as i64 {
            return Err(format!("line {line}: index {index} is outside the {count} vertex(es) declared so far"));
        }
        out.push(at as u32);
    }
    Ok(out)
}

/// Read a **legacy ASCII** VTK (`# vtk DataFile Version …`).
///
/// The classic dialect, cells prefixed by their count — the one
/// `energyml-export` writes. `POLYDATA` (POLYGONS fanned, LINES as chains,
/// VERTICES passed over in silence, the points being already there) and
/// `UNSTRUCTURED_GRID` (CELLS + CELL_TYPES: triangle, quad, polygon,
/// hexahedron as six faces, polyline). `FIELD`, `POINT_DATA` and `CELL_DATA`
/// stop the reading — real data, not errors.
///
/// Refuses by name: a `BINARY` file, an unhandled `DATASET`, the
/// 5.1 layout with `OFFSETS`/`CONNECTIVITY` (to be re-exported as the classic one), an
/// unknown cell type, an index outside the declared points.
pub fn read_vtk(text: &str) -> Result<Geometry, String> {
    let rows: Vec<&str> = text.lines().collect();
    if !rows.first().is_some_and(|first| {
        let lower = first.to_ascii_lowercase();
        lower.starts_with("# vtk datafile") || lower.starts_with("#vtk datafile")
    }) {
        return Err("the first line is not `# vtk DataFile Version …` — this is not a legacy VTK".to_string());
    }
    if rows.iter().any(|row| row.trim().eq_ignore_ascii_case("BINARY")) {
        return Err("this VTK is BINARY; this build reads the ASCII dialect".to_string());
    }
    if rows.iter().any(|row| {
        let word = row.split_whitespace().next().unwrap_or("");
        word.eq_ignore_ascii_case("OFFSETS") || word.eq_ignore_ascii_case("CONNECTIVITY")
    }) {
        return Err("this VTK uses the 5.1 layout (OFFSETS/CONNECTIVITY); re-export it as the classic one".to_string());
    }

    let dataset = rows
        .iter()
        .find_map(|row| {
            let mut words = row.split_whitespace();
            (words.next()?.eq_ignore_ascii_case("DATASET")).then(|| words.next().unwrap_or("").to_string())
        })
        .ok_or_else(|| "no `DATASET` in this VTK".to_string())?;

    let points = vtk_points(&rows)?;
    let count = points.len();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();

    if dataset.eq_ignore_ascii_case("POLYDATA") {
        for ring in vtk_cells(&rows, "POLYGONS", count)? {
            fan(&ring, &mut triangles);
        }
        for chain in vtk_cells(&rows, "LINES", count)? {
            for pair in chain.windows(2) {
                segments.push([pair[0], pair[1]]);
            }
        }
    } else if dataset.eq_ignore_ascii_case("UNSTRUCTURED_GRID") {
        let cells = vtk_cells(&rows, "CELLS", count)?;
        let kinds = vtk_numbers(&rows, "CELL_TYPES");
        for (at, cell) in cells.iter().enumerate() {
            cell_triangles(kinds.get(at).copied().unwrap_or(-1), cell, &mut triangles, &mut segments)?;
        }
    } else {
        return Err(format!("`DATASET {dataset}` is not one this build reads (POLYDATA, UNSTRUCTURED_GRID)"));
    }

    if points.is_empty() {
        return Err("no `POINTS` in this VTK".to_string());
    }
    Ok(one_patch("vtk", points, triangles, segments))
}

/// The six faces of a hexahedron, from VTK's corner order (VTK file formats,
/// "Cell types": VTK_HEXAHEDRON = 12, corners 0-3 the bottom face
/// counter-clockwise seen from above, 4-7 the top face above them). Each face
/// is wound outward, the same table `energyml-export` writes.
const HEXAHEDRON_FACES: [[usize; 4]; 6] =
    [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];

/// What one VTK cell of type `kind` contributes: triangles for an area, segments
/// for a line, nothing for a vertex, and a refusal naming the type for the rest.
/// The one place both dialects decide it, so they cannot disagree.
fn cell_triangles(
    kind: i64,
    cell: &[u32],
    triangles: &mut Vec<[u32; 3]>,
    segments: &mut Vec<[u32; 2]>,
) -> Result<(), String> {
    match kind {
        // 5 triangle, 9 quad, 7 polygon: a fan in all three cases.
        5 | 7 | 9 => fan(cell, triangles),
        // 3 line, 4 polyline: consecutive segments.
        3 | 4 => {
            for pair in cell.windows(2) {
                segments.push([pair[0], pair[1]]);
            }
        }
        // 12 hexahedron: its six faces, in VTK's order.
        12 if cell.len() == 8 => {
            for face in HEXAHEDRON_FACES {
                fan(&face.iter().map(|&i| cell[i]).collect::<Vec<_>>(), triangles);
            }
        }
        // 1 vertex: the points are already there.
        1 => {}
        other => return Err(format!("cell type {other} is not one this build reads")),
    }
    Ok(())
}

/// A VTK_POLYHEDRON's face stream: `numFaces`, then for each face its point
/// count and its points.
fn polyhedron_faces(stream: &[u32], points: usize, triangles: &mut Vec<[u32; 3]>) -> Result<(), String> {
    let bad = || "a polyhedron's `faces` stream does not hold the faces it announces".to_string();
    let (&count, mut rest) = stream.split_first().ok_or_else(bad)?;
    for _ in 0..count {
        let (&size, tail) = rest.split_first().ok_or_else(bad)?;
        if size as usize > tail.len() {
            return Err(bad());
        }
        let (face, tail) = tail.split_at(size as usize);
        if face.iter().any(|i| *i as usize >= points) {
            return Err("a polyhedron names a point this file does not have".to_string());
        }
        fan(face, triangles);
        rest = tail;
    }
    Ok(())
}

/// The fan of a ring, from its first corner.
fn fan(ring: &[u32], out: &mut Vec<[u32; 3]>) {
    for step in 1..ring.len().saturating_sub(1) {
        out.push([ring[0], ring[step], ring[step + 1]]);
    }
}

/// The points of a VTK — **the numbers may spread over several lines**,
/// which the format allows and the exports do.
fn vtk_points(rows: &[&str]) -> Result<Vec<[f64; 3]>, String> {
    let Some(at) = rows.iter().position(|row| row.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case("POINTS")))
    else {
        return Ok(Vec::new());
    };
    let declared: usize = rows[at]
        .split_whitespace()
        .nth(1)
        .and_then(|w| w.parse().ok())
        .ok_or_else(|| "`POINTS` gives no count".to_string())?;
    let mut numbers: Vec<f64> = Vec::with_capacity(declared * 3);
    for row in &rows[at + 1..] {
        if numbers.len() >= declared * 3 {
            break;
        }
        // A line that starts with a keyword stops the harvest: a file
        // that declares more points than it writes stops there rather than
        // eating the next section.
        if row.split_whitespace().next().is_some_and(|w| w.parse::<f64>().is_err()) {
            break;
        }
        numbers.extend(row.split_whitespace().filter_map(|w| w.parse::<f64>().ok()));
    }
    if numbers.len() < declared * 3 {
        return Err(format!("`POINTS` declares {declared} point(s) and the file holds {}", numbers.len() / 3));
    }
    Ok(numbers[..declared * 3].chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect())
}

/// The cells of a section, each prefixed by its count.
fn vtk_cells(rows: &[&str], section: &str, points: usize) -> Result<Vec<Vec<u32>>, String> {
    let Some(at) = rows
        .iter()
        .position(|row| row.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(section)))
    else {
        return Ok(Vec::new());
    };
    let declared: usize = rows[at].split_whitespace().nth(1).and_then(|w| w.parse().ok()).unwrap_or(0);
    let mut flat: Vec<i64> = Vec::new();
    for row in &rows[at + 1..] {
        if row.split_whitespace().next().is_some_and(|w| w.parse::<i64>().is_err()) {
            break;
        }
        flat.extend(row.split_whitespace().filter_map(|w| w.parse::<i64>().ok()));
    }
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while out.len() < declared && cursor < flat.len() {
        let size = flat[cursor].max(0) as usize;
        cursor += 1;
        let mut cell = Vec::with_capacity(size);
        for step in 0..size {
            let Some(&index) = flat.get(cursor + step) else {
                return Err(format!("`{section}` stops in the middle of a cell"));
            };
            if index < 0 || index as usize >= points {
                return Err(format!("`{section}`: index {index} is outside the {points} point(s) declared"));
            }
            cell.push(index as u32);
        }
        cursor += size;
        out.push(cell);
    }
    Ok(out)
}

/// The integers of a section, flat — `CELL_TYPES` is the case.
fn vtk_numbers(rows: &[&str], section: &str) -> Vec<i64> {
    let Some(at) = rows
        .iter()
        .position(|row| row.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(section)))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in &rows[at + 1..] {
        if row.split_whitespace().next().is_some_and(|w| w.parse::<i64>().is_err()) {
            break;
        }
        out.extend(row.split_whitespace().filter_map(|w| w.parse::<i64>().ok()));
    }
    out
}
