//! `energyml sources` and `energyml fetch` — query a public API.
//!
//! **No decision here.** This module parses arguments and prints: what a
//! provider offers, what a request would cost, what a harvest returned. The
//! encoding of a request, the reading of a response and the refusals are in
//! `energyml-providers` — a web front end and the Python bridge get exactly the
//! same ones.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Parser;
use energyml_build::geojson::{Flow, Keep, Wanted};
use energyml_providers::query::{Area, Page, Query, Term};
use energyml_providers::transport::{Native, Transport};
use energyml_providers::{Kind, Providers};

/// `energyml sources` — what this build can query.
#[derive(Parser)]
pub struct SourcesArgs {
    /// A provider, to see its collections and their filters.
    pub provider: Option<String>,
}

/// `energyml fetch` — the quote, then the harvest.
#[derive(Parser)]
pub struct FetchArgs {
    /// The provider, as `energyml sources` names it.
    pub provider: String,
    /// The collection.
    pub collection: String,
    /// An area, as GeoJSON — **the file the map view exports**.
    ///
    /// The first polygon of the document is taken; its coordinates are
    /// WGS 84 degrees, as RFC 7946 requires, and it is the provider that
    /// knows in which order its service wants them.
    #[arg(long, value_name = "FILE.geojson")]
    pub area: Option<PathBuf>,
    /// A condition, `name=value`. Repeatable; all of them are conjoined.
    #[arg(long = "where", value_name = "NAME=VALUE")]
    pub terms: Vec<String>,
    /// Harvest, and write the response **as it is** to this file.
    ///
    /// Without it nothing is written: the command says what it would cost, which
    /// is the repository's shape — `create --plan`, `extract --dry-run`, `rename`
    /// without `--apply`.
    ///
    /// **One page per file**, numbered, as soon as there are several: each
    /// page is a whole GeoJSON document, and gluing them end to end would write
    /// a file nothing reads back.
    #[arg(long, value_name = "FILE.json")]
    pub out: Option<PathBuf>,
    /// Read the stream entity by entity without writing anything, to see what comes.
    #[arg(long)]
    pub read: bool,
    /// Keep only these properties when reading (`--read`), separated by
    /// commas. Without it, everything is kept.
    #[arg(long, value_name = "NAME,NAME")]
    pub only: Option<String>,
    /// Keep no property — what a preview asks for.
    #[arg(long, conflicts_with = "only")]
    pub shapes_only: bool,
    /// How many entities per page.
    #[arg(long, default_value_t = 1000, value_name = "N")]
    pub page: usize,
    /// A ceiling in entities. The harvest stops there, and says so.
    #[arg(long, value_name = "N")]
    pub most: Option<usize>,
}

/// `energyml sources`.
pub fn sources(args: SourcesArgs) -> Result<()> {
    let providers = Providers::builtin();

    let Some(name) = args.provider else {
        for provider in providers.all() {
            println!("{:<12} {}", provider.name(), provider.describe());
        }
        println!("\n`energyml sources <name>` to see what a provider offers");
        return Ok(());
    };

    let provider = providers.get(&name).map_err(|why| anyhow::anyhow!("{why}"))?;
    let about = provider.about();
    println!("{}  —  {}", provider.name(), provider.describe());
    println!("  {}", about.endpoint);
    println!("  {}", about.attribution);
    match &about.licence {
        Some(licence) => println!("  licence: {licence}"),
        // Said rather than left out: an unknown licence is not a
        // permissive licence, and showing an invented credit is worse than none.
        None => println!("  licence: not declared by the service"),
    }

    for collection in provider.collections() {
        println!("\n  {}  ({})", collection.name, collection.feature_type);
        println!("    {}", collection.title);
        println!("    {}", collection.about);
        match &collection.area {
            Some(area) => println!("    --area: {} ({})", area.label, area.property),
            None => println!("    --area: this collection carries no geometry"),
        }
        for filter in &collection.filters {
            let kind = match &filter.kind {
                Kind::Text => "text".to_string(),
                Kind::Number => "number".to_string(),
                Kind::Flag => "yes/no".to_string(),
                Kind::OneOf(values) => format!("one of: {}", values.join(", ")),
            };
            let suggested = match &filter.suggest {
                Some(value) => format!(", suggested: {value}"),
                None => String::new(),
            };
            println!(
                "    --where {}=…  {} [{kind}{}{suggested}]",
                filter.name,
                filter.label,
                if filter.required { ", required" } else { "" }
            );
            println!("        {}", filter.about);
        }
        if !collection.feeds.is_empty() {
            println!("    a harvest feeds: {}", collection.feeds.join(", "));
        }
    }
    Ok(())
}

/// `energyml fetch`.
pub fn fetch(args: FetchArgs) -> Result<()> {
    let providers = Providers::builtin();
    let provider = providers
        .get(&args.provider)
        .map_err(|why| anyhow::anyhow!("{why}"))?;

    let mut terms = Vec::new();
    for term in &args.terms {
        let (name, value) = term
            .split_once('=')
            .with_context(|| format!("`{term}` is not a `name=value` condition"))?;
        terms.push(Term {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
    let area = match &args.area {
        Some(path) => Some(area_of(path)?),
        None => None,
    };
    let query = Query {
        collection: args.collection.clone(),
        area,
        terms,
        most: args.most,
    };

    let transport = Native::new();

    // **The quote first, always.** A kilobyte of response, and it is the
    // only way to announce a cost: in the full response, the counts
    // are written at the end of the document.
    let request = provider.count(&query).map_err(|why| anyhow::anyhow!("{why}"))?;
    let started = Instant::now();
    let answer = transport
        .fetch(&request)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let matched = provider
        .read_count(answer)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let pages = matched.div_ceil(args.page as u64);
    println!(
        "{matched} entity(ies), {pages} page(s) of {} — quote in {:.2}s",
        args.page,
        started.elapsed().as_secs_f64()
    );
    if let Some(most) = args.most
        && (most as u64) < matched
    {
        println!(
            "  the ceiling `--most {most}` will leave {} aside",
            matched - most as u64
        );
    }

    if args.out.is_none() && !args.read {
        println!("\nnothing was harvested — `--out FILE` to write, `--read` to read without writing");
        return Ok(());
    }

    // The list must live as long as the `Keep` that borrows it, hence
    // the two steps: the vector first, what is kept of it afterwards.
    let only: Vec<&str> = args
        .only
        .as_deref()
        .map(|list| list.split(',').map(str::trim).collect())
        .unwrap_or_default();
    let wanted = Wanted {
        properties: match (args.shapes_only, only.is_empty()) {
            (true, _) => Keep::None,
            (false, false) => Keep::Only(&only),
            (false, true) => Keep::All,
        },
    };

    let started = Instant::now();
    let mut written: Vec<PathBuf> = Vec::new();
    let mut read = 0usize;
    let mut refused = 0usize;
    let mut bytes = 0u64;
    let mut page = Page::first(args.page);
    let mut first_byte = None;

    loop {
        let request = provider
            .page(&query, page)
            .map_err(|why| anyhow::anyhow!("{why}"))?;
        let mut answer = transport
            .fetch(&request)
            .map_err(|why| anyhow::anyhow!("{why}"))?;
        if first_byte.is_none() {
            first_byte = Some(started.elapsed());
        }

        let before = read;
        match &args.out {
            // **Writing is copying the stream**, without reading it: it is the
            // cheapest path and the only lossless one — a round trip through our
            // reader would lose what our `Feature` does not model, such as
            // the inner rings of a polygon.
            Some(path) => {
                if answer.status != 200 {
                    bail!("the server answered {} for {}", answer.status, request.url);
                }
                // **One page, one file.** Each page is a whole
                // `FeatureCollection`; concatenating them gives a file
                // nothing reads back, which weighs and is named exactly like
                // a successful harvest. Measured: three pages of the Paris
                // area wrote 71.5 MB of unreadable JSON.
                let path = numbered(path, page.start / page.count.max(1), pages);
                let mut out =
                    std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
                bytes += std::io::copy(&mut answer.body, &mut out).context("writing the harvest")?;
                written.push(path);
                // A page written as it is does not say how many entities it
                // carried; the quote has already said it, and the exact count is that
                // of the file.
                read = (page.start + page.count).min(matched as usize);
            }
            None => {
                let mut most_reached = false;
                let summary = provider
                    .read_page(answer, &wanted, &mut |_| {
                        read += 1;
                        match args.most {
                            Some(most) if read >= most => {
                                most_reached = true;
                                Flow::Stop
                            }
                            _ => Flow::Continue,
                        }
                    })
                    .map_err(|why| anyhow::anyhow!("{why}"))?;
                refused += summary.refused.len();
                for line in summary.refused.iter().take(3) {
                    eprintln!("  refused: {line}");
                }
                if most_reached {
                    break;
                }
                if summary.read == 0 {
                    break;
                }
            }
        }

        if read >= matched as usize || args.most.is_some_and(|most| read >= most) {
            break;
        }
        if read == before && args.out.is_none() {
            break;
        }
        page = page.next();
        print!("\r  {read} / {matched}…");
        let _ = std::io::stdout().flush();
    }
    println!();

    let elapsed = started.elapsed().as_secs_f64();
    match args.out.as_ref() {
        Some(_) => {
            for path in &written {
                println!("written {}", path.display());
            }
            println!(
                "{} file(s), {:.1} MB in {elapsed:.2}s — first byte at {:.2}s",
                written.len(),
                bytes as f64 / (1024.0 * 1024.0),
                first_byte.unwrap_or_default().as_secs_f64()
            );
        }
        None => println!(
            "{read} entity(ies) read{} in {elapsed:.2}s — first byte at {:.2}s",
            match refused {
                0 => String::new(),
                n => format!(", {n} refused"),
            },
            first_byte.unwrap_or_default().as_secs_f64()
        ),
    }
    Ok(())
}

/// The name of a page, numbered **only when there are several**.
///
/// A one-page harvest keeps the name it was given: renaming what the
/// caller named would be a surprise for the ordinary case.
fn numbered(path: &std::path::Path, index: usize, pages: u64) -> PathBuf {
    if pages <= 1 {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path.extension().map(|s| s.to_string_lossy().into_owned());
    let name = match &extension {
        Some(ext) => format!("{stem}.{:0>3}.{ext}", index + 1),
        None => format!("{stem}.{:0>3}", index + 1),
    };
    path.with_file_name(name)
}

/// The first polygon of a GeoJSON, as a ring of degrees.
///
/// The file the map view writes, read by the repository's reader: a web front end
/// and the command line describe an area the same way, and nothing here
/// parses GeoJSON again.
fn area_of(path: &PathBuf) -> Result<Area> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let collection =
        energyml_build::geojson::read(&text).map_err(|why| anyhow::anyhow!("{}: {why}", path.display()))?;
    let ring = collection
        .features
        .iter()
        .find(|one| one.shape == energyml_build::geojson::Shape::Areas)
        .and_then(|one| one.parts.first())
        .with_context(|| format!("{} holds no polygon to use as an area", path.display()))?;
    Ok(Area::Ring(ring.iter().map(|p| (p.lon, p.lat)).collect()))
}
