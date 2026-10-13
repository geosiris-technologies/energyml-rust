//! `energyml select` — which objects of a package fall in a region drawn by rings.
//!
//! Argument parsing and display only: the region, the three tests, the inversion
//! and every refusal are `energyml_build::selection`'s, so this command and the
//! front end answer the same thing. The output is a list of uuids or uris — what a
//! script feeds the next operation (`--uuids`, `--from-list`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use energyml_build::selection::{self, Request, Rings, Test};
use energyml_geometry::{Placement, Topology, scene};
use serde_json::Value;

use crate::{Modes, open};

/// `energyml select` arguments.
#[derive(Parser)]
pub struct SelectArgs {
    /// The EPC file.
    file: PathBuf,
    /// Only objects whose qualified type contains this, case ignored
    /// (`WellboreTrajectory`). Repeatable: any of them. With neither `--type`
    /// nor a list, every object that has a geometry is judged.
    #[arg(long = "type", value_name = "TYPE")]
    types: Vec<String>,
    /// The objects to judge, by uuid or uri, comma separated.
    #[arg(
        long,
        value_name = "U1,U2",
        value_delimiter = ',',
        conflicts_with = "from_list"
    )]
    uuids: Vec<String>,
    /// The objects to judge, one uuid or uri per line (blank lines and `#` ignored).
    #[arg(long, value_name = "FILE", conflicts_with = "uuids")]
    from_list: Option<PathBuf>,
    /// A ring a node must be inside one of (none given: everywhere). Either a
    /// GeoJSON file — each Polygon's first ring counts as an outer ring of this
    /// option and its holes as inner rings — or `PACKAGE.epc#UUID`, a closed
    /// polyline of a package, whose system is then the object's.
    #[arg(long = "outer", value_name = "FILE.geojson|PACKAGE.epc#UUID")]
    outer: Vec<String>,
    /// A ring a node must be inside none of. Same forms; a Polygon's first ring
    /// is the inner ring, and a Polygon with holes is refused (a hole of an
    /// inner ring would put an area back, which a region cannot say).
    #[arg(long = "inner", value_name = "FILE.geojson|PACKAGE.epc#UUID")]
    inner: Vec<String>,
    /// The EPSG code of the rings. Left out: a package ring's own system, and
    /// 4326 (RFC 7946) for a GeoJSON file.
    #[arg(long, value_name = "N")]
    rings_epsg: Option<u32>,
    /// `inside` (every node in the region), `touches` (a node in it, or an edge
    /// through it) or `head` (the first node of a well; anything else is judged
    /// as `touches`, and a note says so).
    #[arg(long, default_value = "inside")]
    test: String,
    /// Swap kept and dropped. Objects that could not be judged stay refused.
    #[arg(long)]
    invert: bool,
    /// What to print for each object kept: `uuid`, `uri`, or the whole answer as `json`.
    #[arg(long, default_value = "uuid")]
    format: String,
}

/// Runs the command.
pub fn run(args: SelectArgs, modes: Modes) -> Result<()> {
    let test = Test::parse(&args.test).map_err(|why| anyhow!("{why}"))?;
    if !["uuid", "uri", "json"].contains(&args.format.as_str()) {
        bail!("format `{}`: one of uuid, uri, json", args.format);
    }
    let store = open(&args.file, &modes)?;
    let rings = read_rings(&args, &modes)?;

    let listed: Vec<String> = match &args.from_list {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect(),
        None => args.uuids.clone(),
    };
    let uris: Vec<String> = if listed.is_empty() {
        selection::candidates(&store, &args.types)
    } else {
        // What was asked is judged as asked; a name the package does not know
        // goes through and comes back refused. `--type` narrows what it knows.
        listed
            .into_iter()
            .filter(|asked| {
                args.types.is_empty()
                    || store.resolve(asked).ok().is_none_or(|found| {
                        args.types
                            .iter()
                            .any(|t| energyml_storage::resource::type_matches(&found.qualified_type, t))
                    })
            })
            .collect()
    };

    let request = Request {
        uris: uris.clone(),
        rings,
        test,
        invert: args.invert,
    };
    let answer = selection::select(&store, &uris, &request);

    for refused in &answer.refused {
        eprintln!("refused  {}  {}", refused.uri, refused.why);
    }
    for note in &answer.notes {
        eprintln!("note: {note}");
    }
    match args.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&answer)?),
        "uri" => answer.kept.iter().for_each(|uri| println!("{uri}")),
        _ => {
            let uuids: HashMap<String, String> = store
                .resources()
                .into_iter()
                .map(|one| (one.uri, one.uuid))
                .collect();
            for kept in &answer.kept {
                println!("{}", uuids.get(kept).map(String::as_str).unwrap_or(kept));
            }
        }
    }
    eprintln!(
        "{} kept, {} dropped, {} refused",
        answer.kept.len(),
        answer.dropped.len(),
        answer.refused.len()
    );
    Ok(())
}

/// The rings the options name, and the one system they share.
fn read_rings(args: &SelectArgs, modes: &Modes) -> Result<Rings> {
    let mut rings = Rings {
        epsg: args.rings_epsg,
        ..Rings::default()
    };
    // The systems the package rings declare: they must agree, or the rings are
    // not one region. A GeoJSON declares none and is WGS 84 (RFC 7946) unless
    // `--rings-epsg` says otherwise.
    let mut declared: Vec<(String, u32)> = Vec::new();
    let mut plain_geojson = false;
    for (as_inner, specs) in [(false, &args.outer), (true, &args.inner)] {
        for spec in specs {
            let (found, epsg, geojson) = ring_source(spec, as_inner, modes)?;
            match epsg {
                Some(epsg) => declared.push((spec.clone(), epsg)),
                // A package ring with no code says nothing; only `--rings-epsg` can.
                None if !geojson && args.rings_epsg.is_none() => {
                    bail!("`{spec}` names no EPSG code: give `--rings-epsg`");
                }
                None => {}
            }
            plain_geojson |= geojson;
            for (is_inner, ring) in found {
                if is_inner {
                    rings.inner.push(ring)
                } else {
                    rings.outer.push(ring)
                }
            }
        }
    }
    for (spec, epsg) in &declared {
        match rings.epsg {
            None => rings.epsg = Some(*epsg),
            Some(known) if known != *epsg => {
                bail!(
                    "`{spec}` is in EPSG:{epsg} but the other rings are in EPSG:{known}: rings of one system, or `--rings-epsg`"
                );
            }
            Some(_) => {}
        }
    }
    if plain_geojson && args.rings_epsg.is_none() {
        match rings.epsg {
            None => rings.epsg = Some(4326),
            Some(known) => bail!(
                "a GeoJSON ring (WGS 84 by RFC 7946) and a package ring in EPSG:{known} are not one system: give `--rings-epsg` to say what the GeoJSON is in"
            ),
        }
    }
    Ok(rings)
}

/// A ring option: the rings it holds as `(is_inner, ring)`, and the system it
/// declares when it declares one.
type Source = (Vec<(bool, Vec<[f64; 2]>)>, Option<u32>, bool);

fn ring_source(spec: &str, as_inner: bool, modes: &Modes) -> Result<Source> {
    if let Some((file, uuid)) = spec.rsplit_once('#') {
        return ring_of_package(Path::new(file), uuid, as_inner, modes)
            .with_context(|| format!("ring `{spec}`"));
    }
    let path = Path::new(spec);
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading the ring file {}", path.display()))?;
    let json: Value =
        serde_json::from_str(&text).with_context(|| format!("{} is not JSON", path.display()))?;
    let mut polygons = Vec::new();
    polygons_of(&json, &mut polygons);
    if polygons.is_empty() {
        bail!("{} holds no Polygon", path.display());
    }
    let mut out = Vec::new();
    for polygon in polygons {
        let mut parts = polygon.into_iter();
        let Some(first) = parts.next() else { continue };
        out.push((as_inner, first));
        let holes: Vec<_> = parts.collect();
        if !holes.is_empty() {
            if as_inner {
                bail!(
                    "{} has a Polygon with holes given as `--inner`: a hole of an inner ring would put an area back",
                    path.display()
                );
            }
            out.extend(holes.into_iter().map(|hole| (true, hole)));
        }
    }
    Ok((out, None, true))
}

/// Every Polygon of a GeoJSON value, as rings of `[x, y]`.
fn polygons_of(json: &Value, out: &mut Vec<Vec<Vec<[f64; 2]>>>) {
    let ring = |value: &Value| -> Option<Vec<[f64; 2]>> {
        value
            .as_array()?
            .iter()
            .map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
            .collect()
    };
    let polygon =
        |value: &Value| -> Option<Vec<Vec<[f64; 2]>>> { value.as_array()?.iter().map(ring).collect() };
    match json["type"].as_str() {
        Some("Polygon") => out.extend(polygon(&json["coordinates"])),
        Some("MultiPolygon") => {
            for each in json["coordinates"].as_array().into_iter().flatten() {
                out.extend(polygon(each));
            }
        }
        Some("Feature") => polygons_of(&json["geometry"], out),
        Some("FeatureCollection") => json["features"]
            .as_array()
            .into_iter()
            .flatten()
            .for_each(|f| polygons_of(f, out)),
        Some("GeometryCollection") => json["geometries"]
            .as_array()
            .into_iter()
            .flatten()
            .for_each(|g| polygons_of(g, out)),
        _ => {}
    }
}

/// The ring a polyline of a package draws, placed, and the system it is in.
fn ring_of_package(file: &Path, uuid: &str, as_inner: bool, modes: &Modes) -> Result<Source> {
    let store = open(&file.to_path_buf(), modes)?;
    // The store's gate: a uri names its entry, a bare uuid the single one, and a
    // uuid held under two versions is refused with its candidates.
    let summary = store
        .resolve(uuid)
        .cloned()
        .map_err(|e| anyhow!("{}: {e}", file.display()))?;
    let object = store
        .get_object(&summary.uri)
        .map_err(|e| anyhow!("reading {}: {e}", summary.uri))?;
    let built = scene::object_of(object.as_reflect(), &store, &summary.uri)
        .map_err(|e| anyhow!("{}: {e}", summary.uri))?;
    let placed = built
        .geometry_in(Placement::Projected)
        .map_err(|e| anyhow!("{e}"))?;
    let patch = placed
        .patches
        .first()
        .ok_or_else(|| anyhow!("it has no geometry"))?;
    // The first polyline of the first patch; any other topology is one line.
    let count = match &patch.topology {
        Topology::Polylines(runs) => runs.first().map(|n| *n as usize).unwrap_or(0),
        _ => patch.points.len(),
    };
    let ring: Vec<[f64; 2]> = patch.points.iter().take(count).map(|p| [p[0], p[1]]).collect();
    let epsg = built.epsg().and_then(|code| u32::try_from(code).ok());
    Ok((vec![(as_inner, ring)], epsg, false))
}
