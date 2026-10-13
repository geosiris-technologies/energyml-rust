//! `energyml create` — one door onto every operation that makes objects.
//!
//! Three verbs, and the same shape as `import` and `translate`:
//!
//! - **`--list`** — *what can I create from this?* Every operation, fit or
//!   not **with its reason**, and what each would ask;
//! - **`--plan`** — *what would it make?* The objects, their identities, the
//!   arrays, what was set aside. Writes nothing, opens no package;
//! - **`--out`** — *do it.* Into `--into` when a package is given, into a new
//!   one otherwise.
//!
//! Like every command in this binary, this one parses arguments and prints.
//! What an operation needs, refuses and makes is decided in `energyml-build`,
//! which has no idea a command line exists — a `Need` becomes an option here
//! without the operation writing a line for it.
//!
//! # The choices come as `--set name=value`
//!
//! One generic option rather than one per parameter, because the parameters
//! are the operation's to declare and `--list` prints them. A value is read as
//! JSON when it is one — a number, `true`, `false` — and as text otherwise, so
//! that `--set stage=Bartonien` needs no quotes and `--set epsg=2154` is a
//! number.
//!
//! # What an input is, the library says
//!
//! Which `--from` is a table and which a GeoJSON, how `top=…` names a slot,
//! how a slot given twice is numbered: `energyml_build::entries`, since 24/09
//! — the recipe runner needs the same answers, and a second copy here would be
//! a second reader of one input. This file keeps the **bytes**: a file off the
//! disk, an address off the network (`ureq`), a package opened once
//! ([`Disk`]).
//!
//! # `--recipe`: a package from a recipe
//!
//! `energyml create --recipe recette.toml --out model.epc` (a recipe file, e.g.
//! `recette.toml`) plays every step of the recipe through this same door, and
//! `energyml_build::recipe` does it; this command reads its arguments and
//! prints the report.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Args;

use energyml_build::entries::{self, Reader};
use energyml_build::operation::{Applies, Choices, Fitness, Identity, Otherwise, Plan, Registry, Slot, When};
use energyml_build::pieces::Pieces;
use energyml_build::recipe;
use energyml_build::source::Source as FromWhere;
use energyml_build::{apply, epc};
use energyml_storage::EpcStore;

/// How the command line gives a table its template — what a refusal names.
const GIVE_TEMPLATE: &str = "`--template <file.json>`";

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// The operation to run — `energyml create --list` names them.
    pub operation: Option<String>,
    /// An input: a GeoJSON file, or an object of a package as `model.epc#<uuid>`
    /// — its geometry, with the CRS and interpretation it already has. Left
    /// out, the operation takes nothing.
    ///
    /// Prefixed by `<slot>=`, it goes into that slot of the operation:
    /// `--from top=model.epc#<uuid> --from base=model.epc#<uuid>`. An
    /// operation with two roles reads them by that name, and without it the
    /// piece is named after the object or the file.
    #[arg(long, value_name = "[SLOT=]FILE | [SLOT=]FILE.epc#UUID")]
    pub from: Vec<String>,
    /// List what can be created from the input, and what each operation asks.
    #[arg(long)]
    pub list: bool,
    /// With `--list`: print what the operations declare as JSON on stdout, and
    /// nothing else — an array of every operation, or one object when an
    /// operation is named (`energyml create --list --json contacts-from-frames`).
    /// Each carries `name`, `describe`, `makes`, `recipe`, `category`, `also`,
    /// `needs` and `takes` (slots: need, default, choices, bounds, cardinality,
    /// `applies`, `group`, `about`), `reports` and `gives` (what it writes, by
    /// role). Declared on the empty bag, so no input is read: `--from` is
    /// ignored.
    #[arg(long, requires = "list")]
    pub json: bool,
    /// A choice, `name=value`. A number, `true`/`false`, or text.
    #[arg(long = "set", value_name = "NAME=VALUE")]
    pub choices: Vec<String>,
    /// Say what the operation would make, and write nothing.
    #[arg(long)]
    pub plan: bool,
    /// Write the result to this EPC.
    #[arg(long, value_name = "FILE.epc")]
    pub out: Option<PathBuf>,
    /// Write the run manifest to this JSON file: what each step wrote, by role,
    /// with the full uris and the collection of the role — what a workflow
    /// chains on (`energyml-run-outputs/1`). Needs `--out`: a run that wrote
    /// nothing has no outputs to list.
    #[arg(long, value_name = "FILE.json")]
    pub manifest: Option<PathBuf>,
    /// An existing package to add to. Without it, a new one is made.
    #[arg(long, value_name = "FILE.epc")]
    pub into: Option<PathBuf>,
    /// What the columns of the `--from` tables mean: an OSDU
    /// `ColumnBasedTableTemplate`, a `File.TabularData`, or the bare list of
    /// column definitions.
    ///
    /// One template for every table given, which is what a set of files
    /// exported by one producer has — the front end applies the first loaded one to
    /// every CSV for the same reason. A table given without it is refused
    /// naming this flag: what a column means is not something to guess.
    #[arg(long, value_name = "FILE.json")]
    pub template: Option<PathBuf>,
    /// Who is writing, for every `Citation`.
    #[arg(long, default_value = "energyml-rust")]
    pub originator: String,
    /// The instant to stamp, `YYYY-MM-DDThh:mm:ssZ`. Defaults to now — exposed
    /// because it is an input: two runs with the same stated time write the
    /// same package, byte for byte.
    #[arg(long)]
    pub now: Option<String>,
    /// Play a recipe: every step, in order, into one package — `--out` to
    /// write it, `--plan` to say what it would make. The recipe states its
    /// operations, inputs, instant and author, so it takes none of the other
    /// options.
    #[arg(
        long,
        value_name = "RECIPE.toml",
        conflicts_with_all = ["operation", "from", "list", "choices", "into", "template", "now"]
    )]
    pub recipe: Option<PathBuf>,
    /// With `--recipe`: leave out the steps of this group (`groupe` in the
    /// recipe) — `--without erreur` plays Lutetia without its `[ERROR]` model.
    /// Repeat for several.
    #[arg(long, value_name = "GROUP", requires = "recipe")]
    pub without: Vec<String>,
}

/// Write the manifest the library built — the face carries it, nothing more.
fn write_manifest(path: &Path, manifest: &energyml_build::manifest::Manifest) -> Result<()> {
    std::fs::write(path, manifest.to_json()).with_context(|| format!("writing {}", path.display()))?;
    println!("run manifest written to {}", path.display());
    Ok(())
}

pub fn run(args: CreateArgs) -> Result<ExitCode> {
    if args.manifest.is_some() && (args.out.is_none() || args.plan) {
        bail!("`--manifest` lists what a run wrote: add `--out <file.epc>` (a `--plan` writes nothing)");
    }
    if let Some(recipe) = args.recipe.as_deref() {
        return from_recipe(recipe, &args.without, args.out.as_deref(), args.plan, args.manifest.as_deref());
    }
    if args.json {
        // The declaration is assembled by the library (`describe`), the front end's
        // module calls the same; nothing but the JSON goes to stdout.
        let registry = Registry::new();
        let out = match args.operation.as_deref() {
            None => energyml_build::describe::declarations(&registry),
            Some(name) => energyml_build::describe::declaration_of(&registry, name)
                .ok_or_else(|| anyhow::anyhow!("no operation is named `{name}` — `energyml create --list` names them"))?,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(ExitCode::SUCCESS);
    }
    // The bytes are read here, what they are is decided in the library.
    let mut disk = Disk::default();
    let template = match args.template.as_deref() {
        None => None,
        Some(path) => Some(
            entries::template(&mut disk, &FromWhere::File(path.display().to_string())).map_err(anyhow::Error::msg)?,
        ),
    };
    let registry = Registry::new();
    // What the operation takes decides whether a file is read as bytes: a slot
    // that takes a document gets them, never a guess from the file's name.
    let takes = args.operation.as_deref().and_then(|name| registry.get(name)).map(|op| op.takes()).unwrap_or_default();
    let mut set_aside: Vec<String> = Vec::new();
    let read = entries::pieces_for(&args.from, template.as_ref(), &mut disk, &mut set_aside, &takes);
    for why in &set_aside {
        eprintln!("  [refused] {why}");
    }
    let pieces = read.map_err(|why| anyhow::Error::msg(why.said(GIVE_TEMPLATE)))?;
    // **What a slot takes when left empty, from the package written into** —
    // `Otherwise::Every`, the same function the front end calls. Without `--into`
    // there is no package to take from, and an empty slot stays empty.
    let pieces = match (args.into.as_deref(), args.operation.as_deref().and_then(|name| registry.get(name))) {
        (Some(into), Some(operation)) if !args.list => {
            let store = energyml_storage::EpcStore::open(into, std::sync::Arc::new(super::registry::all()))
                .with_context(|| format!("opening {}", into.display()))?;
            energyml_build::read::every(&store, &operation.takes(), pieces, energyml_geometry::Placement::Local)
                .map_err(|why| anyhow::anyhow!("{}: {why}", into.display()))?
        }
        _ => pieces,
    };

    let Some(operation) = args.operation.as_deref().filter(|_| !args.list) else {
        list(&registry, &pieces);
        return Ok(ExitCode::SUCCESS);
    };

    let mut choices = Choices::new();
    for one in &args.choices {
        let Some((name, value)) = one.split_once('=') else {
            bail!("`--set {one}`: expected `name=value`");
        };
        let value: serde_json::Value = match serde_json::from_str(value) {
            // A list, for a slot that takes several values: `--set 'units=["Cretaceous","Jurassic"]'`.
            // Without it no such slot could be answered from the command line.
            Ok(json @ (serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Array(_))) => json,
            _ => serde_json::Value::String(value.to_string()),
        };
        choices = choices.set(name, value);
    }
    let when = When { creation: args.now.clone().unwrap_or_else(super::import::now), originator: args.originator.clone() };
    let plan = match registry.plan(operation, &pieces, &choices, &Identity::derived(), &when) {
        Ok(plan) => plan,
        Err(why) => bail!("{why}"),
    };
    print_plan(operation, &plan);

    let Some(out) = args.out else {
        if !args.plan {
            println!("\n(nothing was written: add `--out <file.epc>`, or `--plan` to say so)");
        }
        return Ok(ExitCode::SUCCESS);
    };

    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| out.display().to_string());
    let registry_of_schemas = std::sync::Arc::new(super::registry::all());
    let mut store = epc::open(args.into.as_deref(), &name, registry_of_schemas).with_context(|| match &args.into {
        Some(path) => format!("opening {}", path.display()),
        None => "making an empty package".to_string(),
    })?;
    // **A bare name is in the current folder**, and the store has to be told:
    // without a root, `--out package.epc` wrote the package and refused each of
    // its arrays (`data.h5`: file not found), two thousand lines of refusals
    // for a file that looks as if it came out fine.
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    store.external_files_mut().set_root(parent);
    let made = apply::apply(&plan, &mut store, &when);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .with_context(|| format!("writing {}", out.display()))?;

    println!("\n{} object(s) written to {}", made.written.len(), out.display());
    print_made(&made);
    if let Some(path) = args.manifest.as_deref() {
        let op = registry.get(operation).expect("the plan was made by this operation");
        let step = energyml_build::manifest::step(operation, op, &plan, &made);
        let manifest = energyml_build::manifest::Manifest::new(name.clone(), energyml_build::manifest::data_files(&plan), None, vec![step]);
        write_manifest(path, &manifest)?;
    }
    Ok(if made.written.is_empty() { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

/// What an `apply` did beyond its count: what it replaced, what it wrote
/// beside an object of the same title, what it refused.
fn print_made(made: &apply::Made) {
    /// How many titles a list names before counting the rest: a
    /// list of a thousand lines is not news, it is a wall.
    const SHOWN: usize = 10;

    // **What was replaced is said.** Replaying the same file replaces its
    // own objects, which is intended; overwriting those of another run has the same
    // shape, and that is the news wanted before finding it out in the
    // package.
    if !made.replaced.is_empty() {
        println!("  {} of them replaced an object the package already had", made.replaced.len());
    }
    // **And what was written *beside* another is said too.** Since identity
    // comes from the content, different content adds an object instead of overwriting
    // the previous one. Two
    // objects with the same title read as a duplicate until somebody says
    // the second is a second version.
    if !made.versioned.is_empty() {
        println!(
            "  {} of them were written beside an object of the same title — a second version, not a replacement",
            made.versioned.len()
        );
        for title in made.versioned.iter().take(SHOWN) {
            println!("    {title}");
        }
        if made.versioned.len() > SHOWN {
            println!("    … and {} more", made.versioned.len() - SHOWN);
        }
    }
    for why in &made.refused {
        eprintln!("  [refused] {why}");
    }
    for why in &made.invalid {
        eprintln!("  [not conformant] {why}");
    }
}

/// The command line's I/O: a file off the disk, an address off the network,
/// a package opened once however many of its objects are given.
///
/// **Only the bytes.** What they are — a table, a GeoJSON, an object of a
/// package — is `energyml_build::entries`'s to decide, for this command and
/// for a recipe alike.
#[derive(Default)]
struct Disk {
    opened: BTreeMap<String, EpcStore>,
}

impl Reader for Disk {
    fn text(&mut self, source: &FromWhere) -> std::result::Result<(String, Option<String>), String> {
        match source {
            FromWhere::File(path) => {
                std::fs::read_to_string(path).map(|text| (text, None)).map_err(|why| format!("reading {path}: {why}"))
            }
            // The whole chain, as `main` prints an error: `fetching …: why`.
            FromWhere::Url(url) => fetch(url).map_err(|why| format!("{why:#}")),
        }
    }

    /// A file as it is on the disk — a PDF is not text. An address goes
    /// through [`Reader::text`]: what is fetched here is a GeoJSON or a table.
    fn bytes(&mut self, source: &FromWhere) -> std::result::Result<(Vec<u8>, Option<String>), String> {
        match source {
            FromWhere::File(path) => std::fs::read(path).map(|bytes| (bytes, None)).map_err(|why| format!("reading {path}: {why}")),
            FromWhere::Url(_) => self.text(source).map(|(text, declared)| (text.into_bytes(), declared)),
        }
    }

    fn package(&mut self, path: &str) -> std::result::Result<&EpcStore, String> {
        match self.opened.entry(path.to_string()) {
            std::collections::btree_map::Entry::Occupied(held) => Ok(&*held.into_mut()),
            std::collections::btree_map::Entry::Vacant(room) => {
                let store = EpcStore::open(Path::new(path), Arc::new(super::registry::all()))
                    .map_err(|why| format!("opening {path}: {why}"))?;
                Ok(&*room.insert(store))
            }
        }
    }
}

/// How much of an answer this will hold in memory.
///
/// **A guard against a wrong address, not a judgement about GeoJSON.** `ureq`
/// defaults to 10 MB, which the SIGEOL exports pass on their own — the borehole
/// collection is 82 MB — so the default would have made the feature unusable
/// for the very files it was asked for. What the cap is really for is the
/// mistyped address that serves a video or a disk image: without one, that is a
/// machine filling its memory instead of a refusal.
///
/// It is named in the refusal, because a limit somebody cannot see is a limit
/// they cannot work around.
const MOST_FETCHED: u64 = 512 * 1024 * 1024;

/// One GET, blocking, following redirects.
///
/// `ureq` already refuses a 4xx or 5xx before any body is read, and the address
/// is in the message — which is what matters: a server answering 404 with an
/// HTML page would otherwise reach the GeoJSON reader and be refused as *this
/// is not JSON*, true and useless, sending somebody to look at their file
/// instead of at their address. The explicit check below covers what it lets
/// through.
fn fetch(url: &str) -> Result<(String, Option<String>)> {
    let mut response = ureq::get(url)
        .header("Accept", "application/geo+json, application/json;q=0.9, */*;q=0.1")
        .call()
        .with_context(|| format!("fetching {url}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        bail!("{url}: the server answered {status}");
    }
    let declared = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let text = response
        .body_mut()
        .with_config()
        .limit(MOST_FETCHED)
        .read_to_string()
        .with_context(|| {
            format!("reading the answer from {url} (at most {} MB is held in memory)", MOST_FETCHED / (1024 * 1024))
        })?;
    Ok((text, declared))
}

/// Every operation, fit or not, with its reason and what it would ask.
fn list(registry: &Registry, pieces: &Pieces) {
    println!("Input: {}\n", if pieces.is_empty() { "nothing".to_string() } else { pieces.kinds() });
    for offer in registry.offers(pieces) {
        let operation = offer.operation;
        match &offer.fitness {
            Fitness::Fit => println!("{}  — {}", operation.name(), operation.describe()),
            Fitness::Unfit(why) => println!("{}  — refused: {why}", operation.name()),
        }
        println!("    makes: {}", operation.makes().join(", "));
        // What it **takes**, before what it asks: a slot left empty that takes
        // every frame of `--into` is the news, not a detail.
        for slot in operation.takes() {
            let empty = match (&slot.otherwise, &slot.need) {
                (Otherwise::Every, energyml_build::operation::Need::Input(takes)) if !takes.types.is_empty() => {
                    format!("  [empty: every {} of --into]", takes.types.join(" or "))
                }
                (Otherwise::Every, _) => "  [empty: every object of --into]".to_string(),
                _ => String::new(),
            };
            println!("    --from {}=…  ({}..{}){empty}  {}", slot.name, slot.least, slot.most.map(|m| m.to_string()).unwrap_or("∞".into()), slot.about);
        }
        for slot in &offer.needs {
            println!("    {}", describe_slot(slot));
        }
        println!();
    }
}

/// `--set method=idw|kriging`, or several of them joined by *or* — an `Any`.
fn set_condition(applies: &Applies) -> String {
    match applies {
        Applies::When { slot, is } => format!("--set {slot}={}", is.join("|")),
        Applies::Any(all) => all.iter().map(set_condition).collect::<Vec<_>>().join(" or "),
        _ => String::new(),
    }
}

/// One line per slot: how to set it, what it takes, and the three things it
/// says about itself — when it applies, what its emptiness makes, what it can
/// be copied from.
///
/// The front end hides an inapplicable field; a listing **shows** it and says the
/// condition, because `--list` is read to learn what an operation takes and a
/// missing line reads as *not supported*.
fn describe_slot(slot: &Slot) -> String {
    let required = if slot.is_required() { "required" } else { "optional" };
    let suggested = match &slot.suggested {
        Some(value) => format!(", suggested {value}"),
        None => String::new(),
    };
    let named = |names: &[&str], join: &str| {
        names.iter().map(|name| format!("--set {name}")).collect::<Vec<_>>().join(join)
    };
    let applies = match &slot.applies {
        Applies::Always => String::new(),
        Applies::With(names) => format!(", with {}", named(names, " and ")),
        Applies::Without(names) => format!(", not with {}", named(names, " or ")),
        conditional @ (Applies::When { .. } | Applies::Any(_)) => format!(", only with {}", set_condition(conditional)),
    };
    // A type parameter says what it lists and what it pins: a name typed at a
    // shell has no dropdown to discover it from.
    let picks = match &slot.need {
        energyml_build::operation::Need::Type(choice) => {
            let axis = |c: &energyml_build::types::Constraint, what: &str| match c {
                energyml_build::types::Constraint::Any => String::new(),
                energyml_build::types::Constraint::Prefer(list) => format!(", {what} {} by default", list.join("/")),
                energyml_build::types::Constraint::Only(list) => format!(", {what} {} only", list.join("/")),
            };
            format!(
                "  [types matching `{}`{}{}]",
                choice.filter,
                axis(&choice.package, "package"),
                axis(&choice.version, "version")
            )
        }
        _ => String::new(),
    };
    let otherwise = match &slot.otherwise {
        Otherwise::Creates { types } if !types.is_empty() => format!("  [empty: creates {}]", types.join(" + ")),
        _ => String::new(),
    };
    format!(
        "--set {}=<{}>  ({required}{suggested}{applies}){picks}{otherwise}  {}",
        slot.name,
        slot.need.parameter_kind(),
        slot.about
    )
}

fn print_plan(operation: &str, plan: &Plan) {
    println!("== {operation} — {} ==\n", plan.activity.title);
    println!("{:<16} {:<44} {:<36}  title", "role", "type", "uuid");
    for object in &plan.objects {
        println!("{:<16} {:<44} {:<36}  {}", object.role, object.qualified_type, object.uuid, super::import::truncate(&object.title, 60));
    }
    if !plan.datasets.is_empty() {
        println!();
        for dataset in &plan.datasets {
            let shape: Vec<String> = dataset.dimensions.iter().map(|d| d.to_string()).collect();
            println!("array  {}:{}  [{}]", dataset.file, dataset.path, shape.join(" × "));
        }
    }
    for dropped in &plan.dropped {
        // The count, not the reason alone: "no elevation" once and eight
        // times are two different news.
        println!("  [set aside] {} × {}", dropped.count, dropped.why);
    }
    for note in &plan.says {
        println!("  [{}] {}", note.key, note.detail);
    }
    println!("\n{} object(s), {} array(s)", plan.objects.len(), plan.datasets.len());
}


/// `--recipe`: read the recipe, play it through `energyml_build::recipe`, print
/// what each step did. Nothing else — the steps, their inputs, the workflow and
/// the refusals are the library's.
fn from_recipe(path: &Path, without: &[String], out: Option<&Path>, plan_only: bool, manifest: Option<&Path>) -> Result<ExitCode> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let read = recipe::Recipe::parse(&text).map_err(|stop| anyhow::anyhow!("{}: {stop}", path.display()))?;
    let groups: Vec<&str> = without.iter().map(String::as_str).collect();
    let read = read.without(&groups).map_err(|why| anyhow::anyhow!("{}: {why}", path.display()))?;
    // The recipe's files are relative to its own folder, wherever it is run from.
    let base = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let operations = Registry::new();
    let schemas = Arc::new(super::registry::all());
    let mut disk = Disk::default();
    let played = match (out, plan_only) {
        (Some(_), true) => bail!("`--plan` writes nothing and `--out` writes a package: choose one"),
        (None, true) => recipe::plan(&read, base, &operations, schemas, &mut disk),
        (Some(out), false) => recipe::run(&read, base, &operations, schemas, out, &mut disk),
        (None, false) => bail!("`--recipe` makes a package: add `--out <file.epc>`, or `--plan` to say what it would make"),
    };
    match played {
        Ok(played) => {
            for done in &played.steps {
                print_step(done);
            }
            let held = played.steps.iter().filter(|done| done.held.is_some()).count();
            println!(
                "\nworkflow `{}` ({}) — activity `{}` ({}), the Parent of every step's activity",
                played.workflow.template.title,
                played.workflow.template.uuid,
                played.workflow.activity.title,
                played.workflow.activity.uuid
            );
            println!("{} step(s) played, {held} held", played.steps.len() - held);
            if let (Some(target), Some(out)) = (manifest, out) {
                let epc = out.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_else(|| out.display().to_string());
                write_manifest(target, &played.manifest(&epc, &read.title))?;
            }
            match out {
                Some(out) => println!("written to {}", out.display()),
                None => println!("(nothing was written: `--plan`)"),
            }
            Ok(ExitCode::SUCCESS)
        }
        // What ran before the stop is printed, then the stop itself — by the
        // step's name, through `main`. Nothing was saved.
        Err(stop) => {
            for done in &stop.done {
                print_step(done);
            }
            Err(anyhow::anyhow!("{}: {stop} — nothing was written", path.display()))
        }
    }
}

/// One step of a recipe, as `print_plan` prints an operation's plan — and
/// what applying it did.
fn print_step(done: &recipe::Done) {
    let serves = if done.serves.is_empty() { String::new() } else { format!("  [serves {}]", done.serves.join(", ")) };
    println!("\n== {} — {}{serves} ==", done.step, done.operation);
    if let Some(why) = &done.held {
        println!("  [held] {why}");
        return;
    }
    println!("{}", done.title);
    println!("{:<16} {:<44} {:<36}  title", "role", "type", "uuid");
    for object in &done.objects {
        println!(
            "{:<16} {:<44} {:<36}  {}",
            object.role,
            object.qualified_type,
            object.uuid,
            super::import::truncate(&object.title, 60)
        );
    }
    for why in &done.set_aside {
        println!("  [refused] {why}");
    }
    for dropped in &done.dropped {
        // The count, not the reason alone: "no elevation" once and eight
        // times are two different news.
        println!("  [set aside] {} × {}", dropped.count, dropped.why);
    }
    for note in &done.says {
        println!("  [{}] {}", note.key, note.detail);
    }
    println!("{} object(s), {} array(s)", done.objects.len(), done.arrays);
    if let Some(made) = &done.made {
        print_made(made);
    }
}
