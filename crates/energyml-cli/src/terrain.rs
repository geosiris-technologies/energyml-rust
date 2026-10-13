//! `energyml terrain` — a digital elevation model fetched from a catalogue
//! source and written as a `Grid2dRepresentation`.
//!
//! The use case is draping elevation relief textures over zones in the 3D
//! canvas, with catalogues of sources that the operation's inputs can be
//! drawn from. This file only reads arguments and prints: the catalogue, the
//! choice of source, the window and the reading of the answer are
//! `energyml_providers::terrain`; the writing is the `terrain` operation of
//! `energyml-build`.
//!
//! **The quote first, as `fetch` does**: without `--out`, the command says
//! which source and which window it would ask, and asks nothing.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Parser;
use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_build::{apply, epc};
use energyml_domain::reproject::Reprojector;
use energyml_providers::terrain::{self, TerrainCatalogue};
use energyml_providers::transport::{Native, Transport};

#[derive(Parser)]
pub struct TerrainArgs {
    /// The window, `x_min,y_min,x_max,y_max`, in the system of `--crs`.
    #[arg(long, value_name = "XMIN,YMIN,XMAX,YMAX", allow_hyphen_values = true)]
    pub area: Option<String>,
    /// The system of the window and of the written grid (EPSG code).
    #[arg(long, default_value_t = 2154, value_name = "EPSG")]
    pub crs: u32,
    /// The grid step, in the units of `--crs`.
    #[arg(long, default_value_t = 25.0, value_name = "STEP")]
    pub step: f64,
    /// The source, by its catalogue identifier. Without it, the finest one
    /// that covers the window and that this build can read.
    #[arg(long, value_name = "ID")]
    pub source: Option<String>,
    /// A catalogue of your own, in JSON: its entries come **before** those of the
    /// shipped catalogue, and replace the one that has the same identifier.
    #[arg(long, value_name = "FILE.json")]
    pub catalogue: Option<PathBuf>,
    /// List the catalogue — and, with `--area`, what covers the window.
    #[arg(long)]
    pub list: bool,
    /// Download, and write the grid into this package.
    #[arg(long, value_name = "FILE.epc")]
    pub out: Option<PathBuf>,
    /// An existing package to add the grid to.
    #[arg(long, value_name = "FILE.epc")]
    pub into: Option<PathBuf>,
    /// A choice of the operation, `name=value` — `vertical=5720`, `title=…`,
    /// `crs=…` for a reference system of the `--into` package.
    #[arg(long = "set", value_name = "NAME=VALUE")]
    pub choices: Vec<String>,
    /// The retrieval instant to write — now otherwise.
    #[arg(long)]
    pub now: Option<String>,
    /// Who writes, for each `Citation`.
    #[arg(long, default_value = "energyml-rust")]
    pub originator: String,
}

pub fn run(args: TerrainArgs) -> Result<ExitCode> {
    let mut catalogue = TerrainCatalogue::builtin().map_err(|why| anyhow::anyhow!("{why}"))?;
    if let Some(path) = &args.catalogue {
        let text = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
        let user =
            TerrainCatalogue::parse(&text).map_err(|why| anyhow::anyhow!("{}: {why}", path.display()))?;
        catalogue = catalogue.with_user(user);
    }

    let area = args.area.as_deref().map(parse_area).transpose()?;
    let area_wgs84 = match area {
        Some(area) => Some(to_wgs84(area, args.crs)?),
        None => None,
    };

    if args.list {
        list(&catalogue, area_wgs84);
        return Ok(ExitCode::SUCCESS);
    }
    let (Some(area), Some(area_wgs84)) = (area, area_wgs84) else {
        bail!("`--area x_min,y_min,x_max,y_max` is required (or `--list` to see the catalogue)");
    };

    let ask = terrain::ask(
        &catalogue,
        args.source.as_deref(),
        area,
        args.crs,
        area_wgs84,
        args.step,
    )
    .map_err(|why| anyhow::anyhow!("{why}"))?;
    println!("source      {} — {}", ask.source.id, ask.source.title);
    println!("licence     {}", ask.source.licence);
    println!("attribution {}", ask.source.attribution);
    if let Some(note) = &ask.source.note {
        println!("note        {note}");
    }
    println!(
        "window      [{}, {}, {}, {}] EPSG:{} — {} × {} pixels of {} ({} nodes)",
        ask.bbox[0],
        ask.bbox[1],
        ask.bbox[2],
        ask.bbox[3],
        ask.epsg,
        ask.width,
        ask.height,
        ask.step,
        u64::from(ask.width) * u64::from(ask.height)
    );
    println!("request     {}", ask.request.url);

    let Some(out) = args.out else {
        println!("\nnothing was requested — `--out FILE.epc` to download and write");
        return Ok(ExitCode::SUCCESS);
    };

    let started = Instant::now();
    let answer = Native::new()
        .fetch(&ask.request)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    // Four bytes a node, and the header: a ceiling that names a typo, never the data.
    let most = (u64::from(ask.width) * u64::from(ask.height) * 4 + (1 << 20)) as usize;
    let raster = terrain::read_answer(answer, most).map_err(|why| anyhow::anyhow!("{why}"))?;
    let lattice = terrain::lattice_of(&raster).map_err(|why| anyhow::anyhow!("{why}"))?;
    let holes = lattice.z.iter().filter(|z| !z.is_finite()).count();
    println!(
        "\nreceived {} × {} values in {:.2}s, {} without a value",
        raster.width,
        raster.height,
        started.elapsed().as_secs_f64(),
        holes
    );
    // Why they have no value: the file's own no-data is expected, the others are
    // what the service did and are said, not hidden.
    let (_, voids) = terrain::voids_of(&raster);
    if voids.undeclared + voids.blended_ring > 0 {
        println!(
            "            of which {} below {} m that the file does not declare and {} neighbours of such a pixel (the service's resampling)",
            voids.undeclared,
            terrain::LOWEST_LAND_M,
            voids.blended_ring
        );
    }

    let fetched = args.now.clone().unwrap_or_else(super::import::now);
    let mut choices = Choices::new()
        .set("source", ask.source.id.as_str())
        .set("service", ask.service())
        .set("layer", ask.layer())
        .set("licence", ask.source.licence.as_str())
        .set("attribution", ask.source.attribution.as_str())
        .set("fetched", fetched.as_str());
    // The grid is in `--crs`; a frame is created in it unless one is named.
    let names_a_frame = args.choices.iter().any(|one| one.starts_with("crs="));
    // `sourceEpsg` alone keeps the numbers in that system — metres or degrees;
    // `project` would ask a projection, which a geographic `--crs` is not.
    if !names_a_frame {
        choices = choices.set("sourceEpsg", u64::from(args.crs));
    }
    // The heights go in a data file named after the package, unless one is
    // named: two terrains written side by side would otherwise share `data.h5`.
    if !args.choices.iter().any(|one| one.starts_with("dataFile=")) && args.into.is_none() {
        let stem = out
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "terrain".to_string());
        choices = choices.set("dataFile", format!("{stem}.h5"));
    }
    for one in &args.choices {
        let Some((name, value)) = one.split_once('=') else {
            bail!("`--set {one}`: expected `name=value`");
        };
        let value: serde_json::Value = match serde_json::from_str(value) {
            Ok(json @ (serde_json::Value::Number(_) | serde_json::Value::Bool(_))) => json,
            _ => serde_json::Value::String(value.to_string()),
        };
        choices = choices.set(name, value);
    }

    let pieces = Pieces::empty().with("lattice", Source::Lattice(lattice));
    let pieces = match args.into.as_deref() {
        Some(into) if names_a_frame => {
            let store = energyml_storage::EpcStore::open(into, std::sync::Arc::new(super::registry::all()))
                .with_context(|| format!("opening {}", into.display()))?;
            let takes = Registry::new()
                .get("terrain")
                .map(|op| op.takes())
                .unwrap_or_default();
            energyml_build::read::every(&store, &takes, pieces, energyml_geometry::Placement::Local)
                .map_err(|why| anyhow::anyhow!("{}: {why}", into.display()))?
        }
        _ => pieces,
    };
    let when = When {
        creation: fetched.clone(),
        originator: args.originator.clone(),
    };
    let plan = Registry::new()
        .plan("terrain", &pieces, &choices, &Identity::derived(), &when)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    for note in &plan.says {
        println!("  [{}] {}", note.key, note.detail);
    }

    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| out.display().to_string());
    let mut store = epc::open(
        args.into.as_deref(),
        &name,
        std::sync::Arc::new(super::registry::all()),
    )
    .with_context(|| match &args.into {
        Some(path) => format!("opening {}", path.display()),
        None => "making an empty package".to_string(),
    })?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    store.external_files_mut().set_root(parent);
    let made = apply::apply(&plan, &mut store, &when);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .with_context(|| format!("writing {}", out.display()))?;
    println!("\n{} object(s) written to {}", made.written.len(), out.display());
    for why in &made.refused {
        eprintln!("  [refused] {why}");
    }
    for why in &made.invalid {
        eprintln!("  [non-conforming] {why}");
    }
    Ok(if made.written.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `x_min,y_min,x_max,y_max`.
fn parse_area(text: &str) -> Result<[f64; 4]> {
    let numbers: Vec<f64> = text
        .split(',')
        .map(|one| {
            one.trim()
                .parse::<f64>()
                .with_context(|| format!("`{one}` is not a number"))
        })
        .collect::<Result<_>>()?;
    let Ok(area) = <[f64; 4]>::try_from(numbers) else {
        bail!("`--area {text}` : quatre nombres attendus, x_min,y_min,x_max,y_max");
    };
    Ok(area)
}

/// The area in WGS 84 degrees — the corners and the envelope are the
/// library's (`terrain::area_wgs84`), the geodesy is `energyml-proj`.
fn to_wgs84(area: [f64; 4], epsg: u32) -> Result<[f64; 4]> {
    let backend = energyml_proj::Proj4::new();
    terrain::area_wgs84(area, epsg, |p| {
        backend
            .reproject(p, u64::from(epsg), 4326)
            .map_err(|why| why.to_string())
    })
    .map_err(|why| anyhow::anyhow!("{why}"))
}

/// The catalogue, and with an area, what covers it.
fn list(catalogue: &TerrainCatalogue, area_wgs84: Option<[f64; 4]>) {
    for source in catalogue.sources() {
        let covers = match area_wgs84 {
            Some(area) if source.coverage.contains(area) => "  covers the window",
            Some(_) => "  outside the window",
            None => "",
        };
        let protocol = match &source.protocol {
            terrain::Protocol::WmsRaster { .. } => "wmsRaster",
            terrain::Protocol::Wmts { .. } => "wmts (not read)",
            terrain::Protocol::XyzTerrarium { .. } => "xyzTerrarium (not read)",
            terrain::Protocol::Cog { .. } => "cog (not read)",
        };
        println!(
            "{:<20} {:<10} {:<24} {:>6} m  {}{covers}",
            source.id,
            source.kind.label(),
            protocol,
            source.resolution_m,
            source.licence
        );
    }
    if let Some(area) = area_wgs84 {
        println!(
            "\nwindow in WGS 84: [{:.5}, {:.5}, {:.5}, {:.5}]",
            area[0], area[1], area[2], area[3]
        );
    }
}
