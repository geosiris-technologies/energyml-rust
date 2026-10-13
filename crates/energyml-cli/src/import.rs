//! `energyml import` — a GeoJSON in, an EnergyML package out.
//!
//! Two runs of the same question, and the difference is not a setting:
//!
//! - **`--plan`** — *what would this import make?* Reads the file, groups,
//!   deduplicates, and prints. Writes nothing, opens no package;
//! - **`--out`** — *do it.* The same decisions, kept in an EPC.
//!
//! `--into` names a package to add to; without it a new one is made. The
//! ordinary case wants the first: a BRGM pipeline package already holds the
//! stages these units belong to, and importing into it keeps one model rather
//! than making a second.
//!
//! Like every command in this binary, this one parses arguments and prints.
//! What a GeoJSON becomes is decided in `energyml-build`, which has no idea a
//! command line exists — the whole of it is tested without one.
//!
//! # The one argument that cannot be guessed
//!
//! `--stage`. No GeoJSON names the question it answers, and the stage is what
//! decides whether a point marks a top or a base. It defaults to the file name
//! when that follows the SIGEOL convention (`GeologicUnit_<Stage>_pretty.json`)
//! and is **required** otherwise — never inferred from the first word of a name
//! that means something else, because a wrong default here mislabels every
//! surface in the package and nothing downstream would complain.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;

use energyml_build::{assemble, crs, epc, geojson, sigeol};

#[derive(Parser)]
pub struct ImportArgs {
    /// The GeoJSON file.
    file: PathBuf,
    /// Say what the import would make, and write nothing.
    #[arg(long, conflicts_with = "out")]
    plan: bool,
    /// Write the result to this EPC.
    #[arg(long)]
    out: Option<PathBuf>,
    /// An existing package to import into. Without it, a new one is made.
    #[arg(long)]
    into: Option<PathBuf>,
    /// The stage the export was asked about — what decides top from bottom.
    ///
    /// Read off the file name when it follows `GeologicUnit_<Stage>`, and
    /// required otherwise.
    #[arg(long)]
    stage: Option<String>,
    /// The EPSG code the file's coordinates are in. A GeoJSON is WGS 84 by
    /// definition (RFC 7946), so 4326 unless you know otherwise.
    #[arg(long, default_value_t = 4326)]
    epsg: i64,
    /// Reproject into this EPSG system, in metres.
    ///
    /// **Without it the degrees are written as they stand**, which loses
    /// nothing and cannot be drawn: the horizontal extent is then in degrees and
    /// the vertical in metres, so a 3D view plots a needle — measured at 335:1
    /// on the SIGEOL exports. Nothing in the file lets a reader reconcile the
    /// two, so a scene that is to be looked at needs this.
    ///
    /// `2154` is Lambert-93, `3949` the CC49 zone the BRGM pipeline writes.
    #[arg(long)]
    project: Option<i64>,
    /// The EPSG code of the vertical system, when it is known.
    ///
    /// Left out, the package says **unknown** rather than implying a datum:
    /// RFC 7946 means height above the WGS 84 ellipsoid, and a French producer
    /// usually writes NGF-IGN69 — forty metres apart, both plausible.
    #[arg(long)]
    vertical_epsg: Option<i64>,
    /// The external file the coordinates go into.
    #[arg(long, default_value = "import.h5")]
    data_file: String,
    /// Who is writing, for every `Citation`.
    #[arg(long, default_value = "energyml-rust")]
    originator: String,
    /// The instant to stamp, `YYYY-MM-DDThh:mm:ssZ`. Defaults to now.
    ///
    /// Exposed because it is an **input**: two runs with the same stated time
    /// write the same package, byte for byte, which is what makes an import
    /// reproducible and diffable.
    #[arg(long)]
    now: Option<String>,
}

pub fn run(args: ImportArgs) -> Result<ExitCode> {
    let name = args
        .file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| args.file.display().to_string());

    // The stage: the caller's, then the file name's convention, then a refusal
    // naming both ways out. Never a guess — see the module doc.
    let stage = match args.stage.clone().or_else(|| sigeol::stage_of_file_name(&name)) {
        Some(stage) => stage,
        None => bail!(
            "`{name}` does not follow the `GeologicUnit_<Stage>` convention, so the stage this \
             export answers about cannot be read from it — pass `--stage <Stage>`. It is what \
             decides whether a point marks a top or a base, and no GeoJSON states it."
        ),
    };

    let text = std::fs::read_to_string(&args.file)
        .with_context(|| format!("reading {}", args.file.display()))?;
    let collection = match geojson::read(&text) {
        Ok(collection) => collection,
        Err(why) => bail!("{}: {why}", args.file.display()),
    };
    for why in &collection.refused {
        eprintln!("  [refused] {why}");
    }

    let plan = assemble::plan(&collection, &stage);
    println!("== {name} — stage queried: {stage} ==\n");
    for why in plan.skipped() {
        println!("  [set aside] {why}");
    }
    if !plan.skipped().is_empty() {
        println!();
    }
    if plan.groups.is_empty() {
        // Not an error: a file may legitimately hold nothing for this stage.
        // But it is said, and the exit code says it too — a run that produced
        // nothing must not look like one that produced everything.
        println!("Nothing to import for this stage.");
        return Ok(ExitCode::from(1));
    }

    println!("{:<40} {:>8} {:>8}  surface", "unit", "passes", "points");
    for group in &plan.groups {
        println!(
            "{:<40} {:>8} {:>8}  {} {}",
            truncate(group.unit.name.as_deref().unwrap_or("?"), 40),
            group.features,
            group.points.len(),
            group.stage,
            group.side.label(),
        );
    }
    println!(
        "\n{} group(s), {} pass(es) → {} point(s); {} duplicate(s) set aside",
        plan.groups.len(),
        plan.features(),
        plan.points(),
        plan.features() - plan.points(),
    );

    let Some(out) = args.out else {
        if !args.plan {
            // Neither asked for. Said rather than silently behaving as `--plan`:
            // somebody who meant to write a package and got a listing would find
            // out much later.
            println!("\n(nothing was written: add `--out <file.epc>`, or `--plan` to say so)");
        }
        return Ok(ExitCode::SUCCESS);
    };

    let registry = std::sync::Arc::new(super::registry::all());
    let mut store = epc::open(args.into.as_deref(), &name, registry)
        .with_context(|| match &args.into {
            Some(path) => format!("opening {}", path.display()),
            None => "making an empty package".to_string(),
        })?;
    // The arrays go beside the package, which is where a reader will look.
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        store.external_files_mut().set_root(parent);
    }

    let options = epc::Options {
        name: name.clone(),
        data_file: args.data_file.clone(),
        vertical: match args.vertical_epsg {
            Some(code) => crs::Vertical::Epsg(code),
            None => crs::Vertical::default(),
        },
        target: match args.project {
            Some(code) => crs::Target::Projected(code),
            None => crs::Target::Geographic(args.epsg),
        },
        source_epsg: args.epsg,
        creation: args.now.clone().unwrap_or_else(now),
        originator: args.originator.clone(),
        source: name,
        stage,
    };
    let report = epc::write(&mut store, &plan, &options);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .with_context(|| format!("writing {}", out.display()))?;

    println!("\n{} object(s) written to {}", report.written.len(), out.display());
    for why in &report.notes {
        println!("  [note] {why}");
    }
    for why in &report.refused {
        eprintln!("  [refused] {why}");
    }

    // **What the save made permanent in somebody else's package.**
    //
    // Saving rebuilds every part from the objects in memory, and a part the
    // reader had to repair comes back carrying the repair. On the BRGM package
    // of 07/09, four `BoundaryFeature`s whose `Citation` omitted `Originator`
    // and `Format` were rewritten with those elements **present and empty** — so
    // the file now satisfies the XSD sequence, `energyml check` reports nothing,
    // and the defect that a partner's validator had flagged is invisible.
    //
    // The reading policy states this hazard in so many words
    // (reading policy of `energyml-core`): *writing it back would put values on disk
    // nobody wrote*. It is not this command's place to refuse — the caller asked
    // to import into that package — but it is very much its place to **say so**,
    // once, naming the parts. A defect that stops being reported is worse than
    // one that is.
    let repaired = store.diagnostics();
    if !repaired.is_empty() && args.into.is_some() {
        eprintln!(
            "\n  [warning] {} departure(s) from the specification in the host package were repaired \
             on reading, and the save made them permanent — the invented values are now in the \
             file, and nothing will report them any more:",
            repaired.len()
        );
        for diagnostic in repaired.iter().take(10) {
            eprintln!(
                "    {} {}",
                diagnostic.part,
                diagnostic.message
            );
        }
        if repaired.len() > 10 {
            eprintln!("    … and {} more", repaired.len() - 10);
        }
    }
    // **A refusal is a failure, even when the package came out.** Coordinates
    // that reached no file leave a package that looks complete, and the only
    // moment anybody notices is when a viewer draws nothing.
    Ok(if report.refused.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `YYYY-MM-DDThh:mm:ssZ` for right now.
///
/// The one place this command reads a clock, and `--now` exists so a caller can
/// take even that away.
pub(crate) fn now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Days since the epoch to a civil date — Howard Hinnant's algorithm, the
    // one every implementation uses. Written out rather than pulled in: this is
    // the only date this binary formats, and `chrono` is a dependency for it.
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}

pub(crate) fn truncate(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_string();
    }
    // On characters and not on bytes: these names are French and cutting one
    // mid-`é` would print a replacement character.
    text.chars().take(at.saturating_sub(1)).collect::<String>() + "…"
}
