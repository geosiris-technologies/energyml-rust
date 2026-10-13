//! `energyml` — inspect and validate EnergyML data stores from a shell.
//!
//! ```text
//! energyml list   model.epc              # what is in it
//! energyml check  model.epc              # is it sound
//! energyml show   model.epc <uuid>       # one object, as XML or JSON
//! energyml arrays model.epc              # where the numbers live
//! energyml properties model.epc          # what hangs on what, and how many values
//! energyml colors model.epc              # what is drawn in what colour, and who said so
//! energyml tables model.epc              # the lookup tables, and what they map
//! energyml geometry model.epc            # what can be drawn, and where it is
//! energyml export model.epc --format obj # write it out for another program
//! energyml types                         # what this build can read
//! ```
//!
//! The tool is thin on purpose. Everything it does is a call into
//! `energyml-storage` or a free function of `energyml-core`; if a check is
//! worth having, it belongs in a library where an application can reach it,
//! not in a binary. What lives here is argument parsing and printing.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use energyml_core::json;
use energyml_core::registry::Registry;
use energyml_core::validate::Severity as IssueSeverity;
use energyml_core::xml::XmlWriteOptions;
use energyml_storage::{EpcStore, Severity, graph};

mod cli_reference;
mod create;
pub mod external;
mod galaxy;
mod import;
mod layers;
mod profile;
mod registry;
mod rules;
mod select;
mod series;
mod sources;
mod terrain;

#[derive(Parser)]
#[command(name = "energyml", about, version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Read parts that break the schema instead of refusing them.
    ///
    /// Two kinds of defect are read past. An absent required element comes
    /// back as its default; a value that is *there* and unreadable — a word
    /// outside a closed enumeration, a number that is not a number, an
    /// undefined entity, an `xsi:type` naming an unknown subtype of a concrete
    /// base — is replaced, and the string that was in the file is reported.
    ///
    /// Every one of them is reported. Use it to get at the contents of a
    /// package whose producer wrote something wrong; do not use it for
    /// anything that will be written back, since what comes out is not what
    /// the document said.
    ///
    /// Still refused in both modes: malformed XML, bad encoding, and an
    /// `xsi:type` on a purely abstract choice — there is nothing to fall back
    /// to there.
    #[arg(long, global = true)]
    lenient: bool,

    /// Refuse a part that omits something the schema requires.
    ///
    /// By default an absent required element is filled with its default and
    /// reported, because refusing costs every field the part *did* carry — a
    /// package whose producer left out `Citation.Originator` is otherwise
    /// unreadable in its entirety, geometry included.
    ///
    /// Use this when the objects will be **written back**: read tolerantly,
    /// they hold values nobody wrote, and saving puts those on disk.
    #[arg(long, global = true)]
    strict: bool,

    /// An HDF5 file to read arrays from, whatever the package says.
    ///
    /// Repeatable, and **the name is optional**:
    ///
    /// - `--h5 path/to/data.h5` offers the file without naming it. Offers are
    ///   opened in the order given and the first that holds the dataset wins.
    /// - `--h5 model.h5=path/to/renamed.h5` pins a path to the name the
    ///   document uses, for when you know which name is wrong.
    ///
    /// The name is optional because the whole point is not having to open a
    /// `.rels` in a text editor to find out what your file used to be called.
    /// A file that was renamed still holds every number it held before.
    ///
    /// What is offered is tried **before** what the package names, since
    /// offering one is an explicit instruction and people offer them precisely
    /// when the package is wrong. Anything read from somewhere the document did
    /// not point at is reported.
    #[arg(long = "h5", value_name = "[NAME=]PATH", global = true)]
    h5: Vec<String>,

    /// Show what the operations are doing, on stderr: `info`, `debug`, or a
    /// filter such as `energyml_build=debug,energyml_storage=info`.
    ///
    /// Nothing is shown by default. The same as the `ENERGYML_LOG` environment
    /// variable, which this flag overrides. Spans say how long each step took
    /// (`time.busy`). stdout is never touched, so a script reading a result is
    /// not disturbed.
    #[arg(long = "log", value_name = "FILTER", global = true)]
    log: Option<String>,

    /// How `--log` writes a line: `text` (default) or `json`, one object per
    /// line. Overrides `ENERGYML_LOG_FORMAT`.
    #[arg(long = "log-format", value_name = "FORMAT", global = true, value_parser = ["text", "json"])]
    log_format: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// List the objects an EPC holds.
    List(ListArgs),
    /// Report everything wrong with an EPC: the package, the objects, the references.
    Check(CheckArgs),
    /// Print one object, as XML or JSON.
    Show(ShowArgs),
    /// List the external arrays the objects point at, and the files holding them.
    Arrays(ArraysArgs),
    /// List the properties: what they hang on, what kind they are, how many values.
    Properties(PropertiesArgs),
    /// Resolve the colour of every object, saying which level it came from.
    Colors(ColorsArgs),
    /// Print the lookup tables: their columns, their keys, and their rows.
    Tables(TablesArgs),
    /// Write a package's wellbores as a table — one row per node, the discrete values resolved.
    Wells(WellsArgs),
    /// Read a log, a time series, a flow test or production periods as series:
    /// the index, the channels, what is absent or flagged, and why a reading is refused.
    Series(series::SeriesArgs),
    /// Evaluate the geometry of every representation: points, topology, CRS.
    Geometry(GeometryArgs),
    /// Write representations out to an interchange format.
    Export(ExportArgs),
    /// List the schema types this build can read.
    Types(TypesArgs),
    /// Write the model's documentation as JSON — every type, field and value,
    /// with the `xsd:documentation`, one file per (package, version).
    Docs(DocsArgs),
    /// Work on the parts no parser will touch: list them, take their bytes out,
    /// put repaired bytes back.
    Repair(RepairArgs),
    /// Export the reference graph, for something else to draw.
    Graph(GraphArgs),
    /// Say where each array's numbers really are, and fix the package to agree.
    H5(H5Args),
    /// Correct the schema version a package's parts declare, without reading
    /// what they hold.
    Relabel(RelabelArgs),
    /// Cut a package down to a chosen set of objects, and say what that cost.
    Extract(ExtractArgs),
    /// Rename the external data file a package points at — the fields, the
    /// relationships and the file itself, or refuse.
    Rename(RenameArgs),
    /// List the public APIs this build can query, and what each one offers.
    Sources(sources::SourcesArgs),
    /// Quote — and then harvest — a query against one of them.
    Fetch(sources::FetchArgs),
    /// List the rules: the house rules of this library, where the specification is silent.
    Rules(rules::RulesArgs),
    /// Project the wells of a package on a drawn track: which fall in the band, where along it, on which side.
    Profile(profile::ProfileArgs),
    /// Select the objects of a package that fall in a region drawn by outer and inner rings: a list of uuids or uris to chain into an operation.
    Select(select::SelectArgs),
    /// Read a GeoJSON and write the EnergyML objects it describes.
    Import(import::ImportArgs),
    /// Create objects through one door: list what an input allows, plan it, write it.
    Create(create::CreateArgs),
    /// A digital elevation model from a catalogue source, written as a 2D grid —
    /// the quote without `--out`.
    Terrain(terrain::TerrainArgs),
    /// Write the reference of every command, `cli-reference.md`, from
    /// their definitions — or check it (`--check`). A tool of the repository,
    /// run by a script of each repository; hidden from the help.
    #[command(hide = true, name = "cli-reference")]
    CliReference(cli_reference::CliReferenceArgs),
    /// Write the Galaxy tool XML of an operation from its declaration — what a
    /// Galaxy server runs to call it through the `energyml` image. Hidden: a tool
    /// of the repository, not of a reader of packages.
    #[command(hide = true, name = "galaxy-tool")]
    GalaxyTool(galaxy::GalaxyToolArgs),
    /// Any other word `foo` runs the program `energyml-foo` found on the `PATH`,
    /// with the rest of the line (`etp` runs `retp`, `translate` runs
    /// the translation program).
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

#[derive(Parser)]
struct ExtractArgs {
    /// The EPC file to cut.
    file: PathBuf,
    /// A selection manifest — the JSON the front end writes.
    ///
    /// The decision travels as a file rather than as a list of arguments for
    /// three reasons: it reads, it versions beside the model it curates, and it
    /// replays on the next release of that model. A curation decision is a
    /// document.
    #[arg(long, value_name = "FILE")]
    select: PathBuf,
    /// Where to write the partial package.
    #[arg(long, value_name = "FILE")]
    out: PathBuf,
    /// Leave the array pointers naming the source's data file.
    ///
    /// The default copies the referenced datasets beside the new package, which
    /// is the only way the result is actually smaller — a partial EPC pointing
    /// at the whole source `.h5` is legal and misses the point. Copying needs a
    /// writing backend; without one, this is the honest fallback and the report
    /// says so.
    #[arg(long)]
    keep_arrays: bool,
    /// Say what would happen and write nothing.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Parser)]
struct RelabelArgs {
    /// The EPC file.
    file: PathBuf,
    /// Which package's parts to relabel: `resqml`, `witsml`, `prodml`, `eml`.
    ///
    /// Required, and no default. Every version in an EPC is independent — the
    /// eml parts of a RESQML package have their *own* release — so relabelling
    /// everything would declare a version for objects that never had it.
    #[arg(long, value_name = "PACKAGE")]
    package: String,
    /// What to write, e.g. `2.2_dev3`.
    ///
    /// Spelled the way Energistics names the release: their own distribution
    /// calls the drop `resqml_v2.2_dev3`. The reader compares versions ignoring
    /// separator and case, so `2.2dev3` and `2.2-dev3` are read the same — this
    /// is about what the file will say, not about what can be read back.
    #[arg(long, value_name = "VERSION")]
    schema_version: String,
    /// Only the parts currently on this version, e.g. `2.2`.
    ///
    /// **Required as soon as the package holds more than one**, and that is not
    /// a rare shape: an EPC may legitimately mix releases (EPC §3.2), and the
    /// reference package holds RESQML 2.0.1 objects beside 2.2 ones. Relabelling
    /// on the package name alone would declare 2.0.1 parts to be a 2.2
    /// development drop — the exact corruption this command exists to avoid,
    /// applied by the tool instead of by the producer.
    #[arg(long, value_name = "VERSION")]
    from: Option<String>,
    /// Where to write the corrected package. Without it, nothing is written
    /// and the command reports what it would change.
    #[arg(short, long, value_name = "FILE")]
    out: Option<PathBuf>,
}

#[derive(clap::Args)]
struct H5Args {
    /// The EPC file.
    file: PathBuf,
    /// Write a package whose declarations name the files the search found.
    ///
    /// The point of the whole exercise: a package that has been renamed reads
    /// anyway, but every reader after you pays for the search. This writes the
    /// answer down once.
    #[arg(long, value_name = "OUT.epc")]
    fix: Option<PathBuf>,
    /// Ask for a path for every file the search could not find.
    ///
    /// Off by default so the command is usable in a script. With it, anything
    /// still missing is asked for by name, once, and what you type is used for
    /// the rest of the run.
    #[arg(long)]
    interactive: bool,
}

#[derive(Parser)]
struct RenameArgs {
    /// The EPC file.
    file: PathBuf,
    /// The data file as the package names it today.
    ///
    /// Absent, the package's own declaration answers — when it declares exactly
    /// one file. With several there is no default worth guessing, and the
    /// refusal lists them. That is argument resolution, not a decision about
    /// the package: `rename::plan` is given a name either way.
    #[arg(long, value_name = "NAME")]
    from: Option<String>,
    /// The bare name it should take (EPC §3.2.3: a name, not a path).
    #[arg(long, value_name = "NAME")]
    to: String,
    /// Carry it out. Without it nothing is touched and the quote is printed —
    /// the same two steps as `create --plan` and `extract --dry-run`.
    #[arg(long)]
    apply: bool,
    /// Where to write the package. The file itself when absent.
    ///
    /// The **data file moves either way**: it is one file, shared by whoever
    /// points at it, not a copy made for this package.
    #[arg(long, value_name = "OUT.epc")]
    out: Option<PathBuf>,
}

#[derive(Parser)]
struct GraphArgs {
    /// The EPC file.
    file: PathBuf,
    /// `json` for a viewer, `dot` for Graphviz.
    #[arg(long, default_value = "json")]
    format: String,
    /// Where to write it. Standard output when absent.
    #[arg(short, long, value_name = "FILE")]
    out: Option<PathBuf>,
    /// Only objects whose type contains this, and what they reference.
    #[arg(long, value_name = "SUBSTRING")]
    filter: Option<String>,
}

#[derive(Parser)]
struct RepairArgs {
    /// The EPC file.
    file: PathBuf,
    /// Write every unreadable part's bytes into this directory, under its own
    /// part name. The other half of `--apply`.
    #[arg(long, value_name = "DIR")]
    extract: Option<PathBuf>,
    /// Read a directory laid out like `--extract` wrote it, and stage each file
    /// as the part of the same name. Says, for each, whether it now parses.
    #[arg(long, value_name = "DIR")]
    apply: Option<PathBuf>,
    /// Where to write the repaired package. Without it, `--apply` reports what
    /// would happen and writes nothing.
    #[arg(long, value_name = "FILE")]
    out: Option<PathBuf>,
    /// Stage a part even when the bytes still do not parse.
    ///
    /// Off by default: a repair that made nothing better is more often a
    /// mistake than an intention, and writing it would replace a broken part
    /// with a differently broken one.
    #[arg(long)]
    force: bool,
}

#[derive(Parser)]
struct ListArgs {
    /// The EPC file.
    file: PathBuf,
    /// Skip the citations. Faster, and reads no object XML at all.
    #[arg(long)]
    quick: bool,
    /// Only types whose name contains this, case-insensitively.
    #[arg(long, value_name = "SUBSTRING")]
    filter: Option<String>,
}

#[derive(Parser)]
struct CheckArgs {
    /// The EPC file.
    file: PathBuf,
    /// Only report errors — no warnings, no notes.
    #[arg(long)]
    errors_only: bool,
    /// Skip schema validation of the objects, which is the slow part.
    #[arg(long)]
    no_schema: bool,
    /// A business rule set, as JSON — **added to** the shipped one.
    ///
    /// Added rather than substituted: a house rule and a rule of the
    /// standard answer different questions, and a file that silenced the
    /// technical reference guide's associations by mentioning one rule of
    /// its own would be a surprising way to lose a check.
    #[arg(long, value_name = "FILE")]
    rules: Option<PathBuf>,
    /// Do not run the business rules at all.
    #[arg(long, conflicts_with = "rules")]
    no_rules: bool,
    /// Stop listing after this many findings per section.
    #[arg(long, default_value_t = 50, value_name = "N")]
    max: usize,
}

#[derive(Parser)]
struct ShowArgs {
    /// The EPC file.
    file: PathBuf,
    /// A uuid, or a full ETP URI.
    object: String,
    /// Print JSON instead of XML.
    #[arg(long)]
    json: bool,
    /// Inline the object each reference points at, under `_data`.
    ///
    /// Makes the document self-contained, at the cost of reading the whole
    /// package first.
    #[arg(long, requires = "json")]
    with_data: bool,
    /// Print the part exactly as it is stored, without deserializing it.
    #[arg(long, conflicts_with = "json")]
    raw: bool,
}

#[derive(Parser)]
struct ArraysArgs {
    /// The EPC file.
    file: PathBuf,
    /// Group by file instead of listing every pointer.
    #[arg(long)]
    summary: bool,
    /// Read each array and print its shape, element type and first values.
    ///
    /// Needs a backend: build with `--features hdf5`, or the reads report the
    /// missing capability.
    #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "6")]
    values: Option<usize>,
}

#[derive(Parser)]
struct PropertiesArgs {
    /// The EPC file.
    file: PathBuf,
    /// Only properties hanging on this representation (uuid).
    #[arg(long, value_name = "UUID")]
    representation: Option<String>,
    /// Read the values, and report where their count contradicts the document.
    ///
    /// Needs a backend: build with `--features hdf5`, or the reads report the
    /// missing capability.
    #[arg(long)]
    check: bool,
    /// A `PropertyKindDictionary` XML file, replacing the built-in one.
    ///
    /// The build carries the Energistics eml 2.3 dictionary; pass a newer one,
    /// or a company one, to resolve kinds this build does not know.
    #[arg(long, value_name = "FILE")]
    dictionary: Option<PathBuf>,
}

#[derive(Parser)]
struct ColorsArgs {
    /// The EPC file.
    file: PathBuf,
    /// Only this object (uuid).
    #[arg(long, value_name = "UUID")]
    object: Option<String>,
    /// Do not look at the interpretation and the feature.
    ///
    /// Nothing in the standard says a colour is inherited; this repository
    /// decided it is, in a fallback chain. This switch turns that off and
    /// leaves only what each object carries itself.
    #[arg(long)]
    no_inherit: bool,
    /// Which viewer the colours are for: `3d` (default), `base map`,
    /// `section`, `well correlation`.
    ///
    /// An entry meant for another viewer is skipped rather than drawn.
    #[arg(long, value_name = "KIND", default_value = "3d")]
    viewer: String,
}

#[derive(Parser)]
struct TablesArgs {
    /// The EPC file.
    file: PathBuf,
    /// Print this many rows of each table.
    ///
    /// Needs a backend for a column-based table, whose values live in an
    /// external file; a RESQML 2.0.1 lookup is inline and needs none.
    #[arg(long, value_name = "N", default_value_t = 10)]
    rows: usize,
}

#[derive(Parser)]
struct WellsArgs {
    /// The EPC file.
    file: PathBuf,
    /// Which wellbores, by uuid or by a piece of their title. Left out, all of them.
    #[arg(long = "well", value_name = "UUID|TEXT")]
    wells: Vec<String>,
    /// Write here. Left out, the table goes to the standard output.
    #[arg(long, short, value_name = "FILE.csv")]
    out: Option<PathBuf>,
    /// One file per wellbore, named after it, in the directory `--out` names.
    ///
    /// The way out of a refusal: two wellbores whose columns disagree cannot
    /// share a file, and this says so rather than merging them wrong.
    #[arg(long)]
    separate: bool,
    /// The field separator.
    #[arg(long, value_name = "CHAR", default_value = ",")]
    separator: char,
    /// Write anyway when the columns disagree — the clash is still named.
    #[arg(long)]
    force: bool,
}

#[derive(Parser)]
struct GeometryArgs {
    /// The EPC file.
    file: PathBuf,
    /// Print the first N points of each patch, in local coordinates.
    #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "3")]
    points: Option<usize>,
    /// Apply the CRS: print projected coordinates instead of local ones.
    ///
    /// The local placement only — offset, areal rotation and the depth flip.
    /// Reprojecting between EPSG systems needs a geodesy backend, which this
    /// build does not carry; see `energyml_domain::reproject`.
    #[arg(long)]
    projected: bool,
    /// Place the points *and* convert them to metres.
    ///
    /// Implies `--projected`. Fails, rather than printing a plausible number,
    /// when the CRS states a unit this build cannot convert.
    #[arg(long)]
    metres: bool,
    /// Only representations whose type name contains this, case-insensitively.
    #[arg(long, value_name = "SUBSTRING")]
    filter: Option<String>,
}

#[derive(Parser)]
struct ExportArgs {
    /// The EPC file.
    file: PathBuf,
    /// Which format. `--list` shows what this build offers.
    #[arg(long, default_value = "obj")]
    format: String,
    /// Where to write. A directory gets one file per representation; a file
    /// gets all of them together, where the format allows it.
    #[arg(long, short, value_name = "PATH")]
    out: Option<PathBuf>,
    /// Only representations whose type name contains this, case-insensitively.
    #[arg(long, value_name = "SUBSTRING")]
    filter: Option<String>,
    /// A JSON file of key → dotted path, added to every object's metadata.
    ///
    /// A path may cross references: `RepresentedInterpretation.InterpretedFeature.Citation.Title`
    /// reads through two objects. See `energyml_domain::metadata`.
    #[arg(long, value_name = "FILE")]
    metadata: Option<PathBuf>,
    /// Use the standard metadata keys — identity plus interpretation and
    /// feature — without writing a file.
    #[arg(long, conflicts_with = "metadata")]
    standard_metadata: bool,
    /// Attach the property values to what is written.
    ///
    /// VTK gets one `PointData`/`CellData` array per property, which is what a
    /// viewer colours by; GeoJSON gets a labelled summary, since a feature has
    /// one set of properties and a property has one value per element. Off by
    /// default: it means reading every property of the package and its
    /// external arrays.
    ///
    /// Needs a backend for properties stored in HDF5: build with
    /// `--features hdf5`, or each one reports the missing capability.
    #[arg(long)]
    properties: bool,
    /// Attach only these properties (uuid), implying `--properties`.
    #[arg(long = "property", value_name = "UUID")]
    property: Vec<String>,
    /// Move the values onto these elements, whatever the file hangs them on.
    ///
    /// A per-node temperature looked at on the faces, a per-cell facies looked
    /// at on the points: RESQML says where a property *is*, never where someone
    /// wants to see it. `points` and `cells` transfer; leaving this out writes
    /// each property where the file put it.
    ///
    /// The transfer is a real computation and it is named, not guessed — see
    /// `--transfer-method`.
    #[arg(long, value_name = "SCOPE", value_parser = ["points", "cells"])]
    properties_on: Option<String>,
    /// Sweep every polyline into a solid tube of this radius.
    ///
    /// A well drawn as a line is one pixel wide at any distance that shows the
    /// model. A **radius**, not a diameter — every sweep in every renderer takes
    /// a radius, and converting silently at one end of a pair of interfaces is
    /// how a well comes out twice too fat.
    ///
    /// The path is swept as it stands: the points are already the evaluated
    /// curve (`parametric.rs` evaluates a minimum-curvature well path rather
    /// than straightening it), so smoothing them again would draw a path
    /// through positions the file does not state.
    #[arg(long, value_name = "RADIUS")]
    tube: Option<f64>,
    /// How many faces go round the tube — the arity of the cross-section.
    ///
    /// Three is a triangular prism and reads as a mistake; past about sixteen
    /// nothing on screen changes and every vertex is paid for.
    #[arg(long, value_name = "N", default_value_t = 8)]
    tube_sides: usize,
    /// How several values become one when they are moved.
    ///
    /// `average` for a measurement, and it is the default. **`majority` for a
    /// category**: the mean of facies 0 and facies 3 is 1.5, which is not a
    /// facies at all, so a discrete property averaged onto the faces paints a
    /// confident picture of a rock that is not there. `minimum` and `maximum`
    /// are there for a conservative reading of a threshold.
    #[arg(long, value_name = "METHOD", default_value = "average")]
    transfer_method: String,
    /// Reproject into this EPSG system. `4326` is WGS 84 longitude/latitude,
    /// `3857` Web Mercator. GeoJSON needs one and defaults to 4326.
    ///
    /// The elevation is **not** converted: it stays in the vertical CRS it was
    /// written in. See `energyml_proj` for why.
    #[arg(long, value_name = "CODE")]
    epsg: Option<u64>,
    /// A JSON file of `"epsg code": "proj4 string"`, replacing the built-in
    /// definitions.
    ///
    /// Where to reach for when the built-in transformation is not the one your
    /// survey specifies — it is a single Helmert per datum, good to a few
    /// metres, and sometimes that is not good enough.
    #[arg(long, value_name = "FILE")]
    crs_definitions: Option<PathBuf>,
    /// Convert even from a system whose definition states no datum.
    ///
    /// Off by default, and worth leaving off: the shift to WGS 84 is then
    /// silently skipped, which on a pre-GPS datum puts the model a hundred
    /// metres or so from where it belongs, with every coordinate still looking
    /// entirely reasonable.
    #[arg(long)]
    allow_silent_datum: bool,
    /// Write time-domain objects, stating what one second of vertical travel
    /// time stands for in metres.
    ///
    /// Without it a representation in a seismic time system is **refused**, and
    /// that is the honest default: the package carries no velocity model, and
    /// RESQML puts the time kind — one-way or two-way — outside the CRS, so a
    /// real depth is half or double whatever this produces.
    ///
    /// What it buys is a mesh you can look at, with the equivalence written
    /// into the file beside the geometry. `2000` is a mid-range sediment
    /// velocity and a reasonable place to start.
    #[arg(long, value_name = "M_PER_S")]
    time_equivalence: Option<f64>,
    /// Write coordinates as stored, reprojecting nothing.
    ///
    /// For GeoJSON, which is WGS 84 by definition: with this flag the file
    /// carries the native projected coordinates instead, plus the legacy
    /// `crs` member naming the EPSG code when there is one (QGIS and GDAL
    /// honour it), and objects that name no CRS at all are written too. The
    /// file is knowingly non-conformant — useful for looking at a package
    /// whose georeferencing is missing or under repair.
    #[arg(long, conflicts_with = "epsg")]
    native_crs: bool,
    /// Draw these types in this form: `--as PATTERN=SHAPE`, repeatable.
    ///
    /// The pattern matches anywhere in the qualified type, case-insensitively —
    /// `Grid2d`, `resqml20.`, `IjkGridRepresentation`, or `*` for everything.
    ///
    /// The shape is `points`, `lines`, `surface`, `volume` or `auto`, or one of
    /// the two **summaries** — `outline` for the convex hull of the object as a
    /// single polygon, `centroid` for the mean of its nodes as a single point.
    /// Those two answer "where is this" rather than "what is this", and carry
    /// no property: they are what OSDU's SpatialArea and SpatialPoint ask for.
    ///
    /// Rules are tried in the order given and the first match wins, so a
    /// general rule and an exception compose in one line:
    ///
    ///   --as Grid2d=surface --as '*=points'
    ///
    /// A shape an object cannot take is an **error**, not a fallback: asking
    /// for a PolylineSet as a surface fails by name, because a line is not a
    /// boundary unless it closes and RESQML states closure somewhere this build
    /// does not read.
    #[arg(long = "as", value_name = "PATTERN=SHAPE")]
    shapes: Vec<String>,
    /// Print the formats this build can write, and stop.
    #[arg(long)]
    list: bool,
}

#[derive(Parser)]
struct DocsArgs {
    /// Write one file per (package, version) plus an `index.json`, into this
    /// directory. Without it, the JSON goes to stdout.
    ///
    /// Files are named `<package>-<version>.json` — `resqml-2.2.json` — so
    /// "only resqml" or "only resqml 2.2" is choosing files, not a query:
    /// the index says what exists, each file is self-contained.
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Only this package: `resqml`, `witsml`, `prodml`, `eml`.
    #[arg(long)]
    package: Option<String>,
    /// Only this version, e.g. `2.2` — the directory spelling, so RESQML
    /// 2.0.1 is `2.0.1`.
    #[arg(long)]
    version: Option<String>,
}

#[derive(Parser)]
struct TypesArgs {
    /// Only this package: `resqml`, `witsml`, `prodml`, `eml`.
    #[arg(long)]
    package: Option<String>,
    /// Describe one type instead of listing them: its fields, and for each
    /// what may legally go in it.
    ///
    /// A name (`TriangulatedSetRepresentation`), a qualified type
    /// (`resqml22.TriangulatedSetRepresentation`), or `Type.Field` for one
    /// field. Answers the two questions the schema makes hard: which concrete
    /// types a field accepts, and which values an enumeration allows — with
    /// open ones marked, since those take any string as well.
    #[arg(long, value_name = "TYPE[.FIELD]")]
    describe: Option<String>,
}

/// Run the `energyml` command line: parse the arguments, do the work, return
/// the exit code. Both binaries (`energyml` and its short alias `enyml`) call
/// this and nothing else.
pub fn run() -> ExitCode {
    match try_run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn try_run() -> Result<ExitCode> {
    let mut command = Cli::command();
    // The external commands found on the `PATH` close the `--help`; a program
    // that is not installed is simply not listed.
    if let Some(section) = external::help_section() {
        command = command.after_help(section);
    }
    let cli = Cli::from_arg_matches(&command.get_matches()).unwrap_or_else(|e| e.exit());
    // **The one place logging is installed.** Nothing asked, nothing installed:
    // every command prints what it printed.
    let format = cli
        .log_format
        .as_deref()
        .map(|f| f.parse::<energyml_log::Format>())
        .transpose()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    energyml_log::init(cli.log.as_deref(), format).map_err(|e| anyhow::anyhow!("{e}"))?;
    let modes = Modes {
        lenient: cli.lenient,
        strict: cli.strict,
        h5: cli.h5.clone(),
    };
    match cli.command {
        Command::List(args) => list(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        // The only command with a meaningful exit code: a script gates on it.
        Command::Check(args) => check(args, modes.clone()),
        Command::Show(args) => show(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Arrays(args) => arrays(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Properties(args) => properties(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Colors(args) => colors(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Tables(args) => tables(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Wells(args) => wells(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Series(args) => series::run(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Geometry(args) => geometry(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Export(args) => export(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Types(args) => types(args).map(|()| ExitCode::SUCCESS),
        Command::Docs(args) => docs(args).map(|()| ExitCode::SUCCESS),
        Command::Repair(args) => repair(args, modes.clone()),
        Command::Graph(args) => graph_command(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::H5(args) => h5_command(args, modes.clone()),
        Command::Relabel(args) => relabel(args, modes.clone()),
        Command::Extract(args) => extract_command(args, modes.clone()),
        Command::Rename(args) => rename_command(args, modes.clone()),
        Command::Sources(args) => sources::sources(args).map(|()| ExitCode::SUCCESS),
        Command::Fetch(args) => sources::fetch(args).map(|()| ExitCode::SUCCESS),
        Command::Rules(args) => rules::run(args).map(|()| ExitCode::SUCCESS),
        Command::Profile(args) => profile::run(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Select(args) => select::run(args, modes.clone()).map(|()| ExitCode::SUCCESS),
        Command::Import(args) => import::run(args),
        Command::Create(args) => create::run(args),
        Command::Terrain(args) => terrain::run(args),
        Command::CliReference(args) => cli_reference::run::<Cli>(args),
        Command::GalaxyTool(args) => galaxy::run(args),
        Command::External(words) => external::run(&words),
    }
}

/// Open an EPC with the reading switches of `modes`, every schema release
/// registered.
pub fn open(file: &PathBuf, modes: &Modes) -> Result<EpcStore> {
    let mut store = EpcStore::open(file, Arc::new(registry::all()))
        .with_context(|| format!("opening {}", file.display()))?;
    store.set_lenient(modes.lenient);
    store.set_strict(modes.strict);
    for spec in &modes.h5 {
        // `name=path`, or just `path`. Split on the **first** `=` so a path
        // containing one still works on the right-hand side; a Windows drive
        // letter uses `:` and is unaffected either way.
        match spec.split_once('=') {
            Some((name, path)) if !name.is_empty() && !path.is_empty() => {
                store.external_files_mut().set(name, path);
            }
            _ => {
                store.external_files_mut().offer(spec.trim_start_matches('='));
            }
        }
    }
    Ok(store)
}

/// The two reading switches, carried together so a command cannot honour one
/// and forget the other.
#[derive(Clone, Default)]
pub struct Modes {
    /// Repair values that are there and wrong.
    pub lenient: bool,
    /// Refuse parts that omit something required.
    pub strict: bool,
    /// External files the caller offered or pinned, in the order given.
    pub h5: Vec<String>,
}

/// Report the parts a `get_object` refused.
///
/// Every command that walks the package hits this: an object whose part breaks
/// the schema cannot be deserialized, and dropping it silently is how a run
/// ends up reporting zero of something the file plainly contains.
///
/// The hint at the end names the switch that would actually help, which depends
/// on why the part was refused — and after 02/08 an *absent* element is not one
/// of the reasons unless the caller asked for `--strict`.
fn report_unreadable(unreadable: &[String], modes: &Modes, limit: usize) {
    if unreadable.is_empty() {
        return;
    }
    eprintln!("{} object(s) could not be read:", unreadable.len());
    for line in unreadable.iter().take(limit) {
        eprintln!("  {line}");
    }
    if unreadable.len() > limit {
        eprintln!("  … and {} more", unreadable.len() - limit);
    }
    if modes.strict && unreadable.iter().any(|line| line.contains("missing required")) {
        eprintln!(
            "(these are absent elements, refused because of `--strict`; drop it to read them \
             with defaults for what the document does not carry)"
        );
    } else if !modes.lenient {
        eprintln!("(retry with `--lenient` to repair values that are there and unreadable)");
    }
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn list(args: ListArgs, modes: Modes) -> Result<()> {
    let store = open(&args.file, &modes)?;

    let mut shown = 0usize;
    for summary in store.resources() {
        if let Some(filter) = &args.filter
            && !energyml_storage::resource::type_matches(&summary.qualified_type, filter)
        {
            continue;
        }
        shown += 1;
        if args.quick {
            println!("{}  {}", summary.uuid, summary.qualified_type);
            continue;
        }
        match store.header(&summary.uri) {
            Ok(header) => println!(
                "{}  {:<44}  {}",
                summary.uuid,
                summary.qualified_type,
                header.titled()
            ),
            // A part whose citation cannot be read is still a part. Saying so
            // beats dropping the row.
            Err(e) => println!(
                "{}  {:<44}  <unreadable: {e}>",
                summary.uuid, summary.qualified_type
            ),
        }
    }

    eprintln!();
    eprintln!("{shown} object(s) in {}", args.file.display());
    let diagnostics = store.diagnostics().len();
    if diagnostics > 0 {
        eprintln!("{diagnostics} diagnostic(s) — run `energyml check` to see them");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

/// Three independent things can be wrong with an EPC, and they are found in
/// three different ways. Conflating them into one number tells nobody what to
/// fix, so each gets its own section.
fn check(args: CheckArgs, modes: Modes) -> Result<ExitCode> {
    use energyml_domain::check;

    let store = open(&args.file, &modes)?;

    println!("== {} ==", args.file.display());

    // **Nothing here judges anything, and nothing below computes.** The
    // reading and the report — including which finding counts as an error —
    // are `energyml-domain::check` (`of_store`), so the front end and the Python
    // bridge answer the same question with the same arithmetic. What is left
    // here is printing.
    let extra_rules = match &args.rules {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading the rule set `{}`", path.display()))?;
            // An unreadable set is an error that **stops**: carrying on would
            // display "0 violations" for rules nobody evaluated, which reads
            // like a healthy file.
            energyml_domain::rules::parse(&text).map_err(anyhow::Error::msg)?
        }
        None => Vec::new(),
    };
    let own = extra_rules.len();
    let report = check::of_store(
        &store,
        &check::Options {
            schema: !args.no_schema,
            rules: !args.no_rules,
            extra_rules,
        },
    );

    // 1 — the package. Departures from OPC and from the EPC conventions,
    // gathered while the archive was indexed.
    let shown: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| !args.errors_only || d.severity == Severity::Warning)
        .collect();
    println!("\npackage: {} diagnostic(s)", report.diagnostics.len());
    for diagnostic in shown.iter().take(args.max) {
        println!("  {diagnostic}");
    }
    if shown.len() > args.max {
        println!("  … and {} more", shown.len() - args.max);
    }

    // 2 — the objects, against their schema: facets, cardinality, patterns.
    println!(
        "\nobjects: {} read, {} unreadable",
        report.read,
        report.unreadable.len()
    );
    for (uri, why) in report.unreadable.iter().take(args.max) {
        println!("  {uri}: {why}");
    }
    if report.unreadable.len() > args.max {
        println!("  … and {} more", report.unreadable.len() - args.max);
    }

    if report.schema_checked {
        let issues: Vec<_> = report
            .schema
            .iter()
            .filter(|issue| !args.errors_only || issue.severity == IssueSeverity::Error)
            .collect();
        for issue in issues.iter().take(args.max) {
            println!(
                "  [{:?}] {} {}: {} ({})",
                issue.severity, issue.uuid, issue.path, issue.message, issue.code
            );
        }
        println!("\nschema: {} issue(s)", issues.len());
        if issues.len() > args.max {
            println!("  … and {} more", issues.len() - args.max);
        }
    } else {
        println!("\nschema: skipped (--no-schema)");
    }

    // 3 — the properties, against the rules the technical reference guides
    // state and no XSD can: an abstract property kind used directly, a
    // deprecated one, a unit outside its kind's quantity class.
    let lines: Vec<String> = report
        .properties
        .iter()
        .filter(|issue| !args.errors_only || issue.is_a_defect)
        .map(|issue| format!("  {} {}", issue.uuid, issue.says))
        .collect();
    println!("\nproperties: {} issue(s)", lines.len());
    for line in lines.iter().take(args.max) {
        println!("{line}");
    }
    if lines.len() > args.max {
        println!("  … and {} more", lines.len() - args.max);
    }
    if report.properties_unresolved > 0 {
        println!(
            "  ({} whose property kind could not be resolved, so not checked)",
            report.properties_unresolved
        );
    }

    // 3 ter — the counts: what a property holds against what it hangs on.
    // Not run with `--no-rules`; an unjudgeable count is named, never silent.
    if report.rules_checked {
        println!(
            "
property counts: {} error(s), {} remark(s)",
            report.counts.iter().filter(|one| one.is_error).count(),
            report.counts.iter().filter(|one| !one.is_error).count()
        );
        for one in report
            .counts
            .iter()
            .filter(|one| !args.errors_only || one.is_error)
            .take(args.max)
        {
            println!(
                "  [{}] {} {}",
                if one.is_error { "error" } else { "remark" },
                one.uuid,
                one.says
            );
        }
        if report.counts.len() > args.max {
            println!("  … and {} more", report.counts.len() - args.max);
        }
        if report.counts_unchecked > 0 {
            println!(
                "  ({} that could not be counted, so not checked)",
                report.counts_unchecked
            );
        }
    }

    // 3 bis — the **business** constraints: what the technical guides
    // associate and that no XSD can enforce. Three outcomes, not two: the
    // inapplicable is silent by construction, a violation is a fault of the
    // file, and a rule that could not be evaluated is *uncheckable* — never a
    // pass.
    if report.rules_checked {
        let violations = report.rules.iter().filter(|one| one.read.is_some()).count();
        let unverifiable = report.rules.iter().filter(|one| one.why.is_some()).count();
        let mut lines: Vec<String> = Vec::new();
        for finding in &report.rules {
            match (&finding.read, &finding.why) {
                (Some(read), _) => lines.push(format!(
                    "  {} {} [{}] — {} (read: {read})",
                    finding.uuid,
                    finding.rule,
                    if finding.gravity == energyml_domain::rules::Gravity::Error {
                        "error"
                    } else {
                        "warning"
                    },
                    finding.says
                )),
                (None, Some(why)) if !args.errors_only => lines.push(format!(
                    "  {} {} — not checkable: {why}",
                    finding.uuid, finding.rule
                )),
                _ => {}
            }
        }
        println!(
            "\nbusiness rules: {violations} violation(s) over {} rule(s){}",
            report.rule_count,
            if own > 0 {
                format!(" ({own} of your own)")
            } else {
                String::new()
            }
        );
        for line in lines.iter().take(args.max) {
            println!("{line}");
        }
        if lines.len() > args.max {
            println!("  … and {} more", lines.len() - args.max);
        }
        if unverifiable > 0 && args.errors_only {
            println!("  ({unverifiable} could not be checked; drop --errors-only to see why)");
        }
    } else {
        println!("\nbusiness rules: skipped (--no-rules)");
    }

    // 4 — the references. Nothing else finds these: each DOR is well-formed on
    // its own, and no XSD validator knows what the package contains.
    let reportable = report.reportable_references();
    let informational = report.catalogue_references();
    let shown: Vec<_> = reportable
        .iter()
        .filter(|i| !args.errors_only || i.is_error())
        .collect();
    println!("\nreferences: {} issue(s)", reportable.len());
    for issue in shown.iter().take(args.max) {
        println!("  {} {issue}", issue.source);
    }
    if shown.len() > args.max {
        println!("  … and {} more", shown.len() - args.max);
    }
    if !informational.is_empty() && !args.errors_only {
        println!(
            "\nreferences to the Energistics catalogue: {} (not carried by the package, as expected)",
            informational.len()
        );
        for issue in informational.iter().take(args.max.min(3)) {
            println!("  {} {issue}", issue.source);
        }
        if informational.len() > args.max.min(3) {
            println!("  … and {} more", informational.len() - args.max.min(3));
        }
        println!("  whether each uuid is in the dictionary is the `properties:` section's job");
    }

    // 5 — the index against the objects. The `.rels` are a cache of the graph
    // the DORs describe; §3.4.1 makes both directions mandatory, and nothing
    // keeps the cache honest. None of this is fatal — the reader follows the
    // DORs — but a tool that trusts the index draws a different graph from the
    // one the data has, and there are such tools.
    if !args.errors_only {
        println!(
            "\nrelationships: {} disagreement(s) with the objects",
            report.relationships.len()
        );
        for issue in report.relationships.iter().take(args.max) {
            println!("  {issue}");
        }
        if report.relationships.len() > args.max {
            println!("  … and {} more", report.relationships.len() - args.max);
        }
    }

    // **The totals are the report's, not this command's.** They say what the
    // package holds and not what this run chose to display, so `--errors-only`
    // no longer reports "0 warning(s)" about a file that has them.
    println!("\n{} error(s), {} warning(s)", report.errors(), report.warnings());
    Ok(if report.is_sound() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

fn show(args: ShowArgs, modes: Modes) -> Result<()> {
    let store = open(&args.file, &modes)?;

    if args.raw {
        let bytes = store.part_bytes(&args.object)?;
        print!("{}", String::from_utf8_lossy(&bytes));
        return Ok(());
    }

    let object = store.get_object(&args.object)?;
    if !args.json {
        println!(
            "{}",
            energyml_core::registry::to_part(object.as_ref(), XmlWriteOptions::default())?
        );
        return Ok(());
    }

    // `_data` costs a full read of the package, so it is opt-in rather than a
    // default that quietly turns `show` into a minute-long command.
    if !args.with_data {
        println!("{}", json::write_string(object.as_reflect(), true)?);
        return Ok(());
    }
    // `--with-data` resolves the references of the object being shown against
    // the rest of the package, so a part that will not deserialize is a
    // reference that will silently come back **unresolved** — the `_data`
    // member simply absent, indistinguishable from a reference that points at
    // nothing. Named on stderr, so the JSON on stdout stays pipeable.
    let mut others = Vec::new();
    let mut unreadable = Vec::new();
    for summary in store.resources() {
        match store.get_object(&summary.uri) {
            Ok(object) => others.push(object),
            Err(e) => unreadable.push(format!("{}  {}  {e}", summary.uuid, summary.qualified_type)),
        }
    }
    report_unreadable(&unreadable, &modes, 10);
    let source = json::ObjectsInMemory::new(others.iter().map(|o| o.as_reflect()));
    println!("{}", json::write_string_with(object.as_reflect(), &source, true)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// arrays
// ---------------------------------------------------------------------------

fn arrays(args: ArraysArgs, modes: Modes) -> Result<()> {
    let store = open(&args.file, &modes)?;
    let mut by_file: Vec<(String, usize)> = Vec::new();
    let mut total = 0usize;
    let mut unresolved = 0usize;
    let mut unreadable = 0usize;
    // A part that will not deserialize points at no arrays, which is
    // indistinguishable from an object that genuinely holds none.
    let mut unparsed: Vec<String> = Vec::new();

    for summary in store.resources() {
        let object = match store.get_object(&summary.uri) {
            Ok(object) => object,
            Err(e) => {
                unparsed.push(format!("{}  {}  {e}", summary.uuid, summary.qualified_type));
                continue;
            }
        };
        for location in store.arrays_of(object.as_reflect()) {
            total += 1;
            if !location.is_resolved() {
                unresolved += 1;
            }
            let file = if location.is_resolved() {
                location.file.clone()
            } else {
                "<unresolved>".to_string()
            };
            match by_file.iter_mut().find(|(name, _)| *name == file) {
                Some((_, count)) => *count += 1,
                None => by_file.push((file.clone(), 1)),
            }
            if args.summary {
                continue;
            }
            print!(
                "{}  {}  {}:{}",
                summary.uuid, location.field_path, file, location.path_in_file
            );
            match args.values {
                None => println!(),
                Some(limit) => match store.read_array(&location) {
                    Ok(data) => println!("  {}", preview(&data, limit)),
                    Err(e) => {
                        unreadable += 1;
                        println!("  <{e}>");
                    }
                },
            }
        }
    }

    if args.summary || total == 0 {
        for (file, count) in &by_file {
            println!("{count:>6}  {file}");
        }
    }
    eprintln!();
    eprintln!(
        "{total} array pointer(s), {} file(s), {unresolved} unresolved",
        by_file.len()
    );
    if args.values.is_some() && unreadable > 0 {
        eprintln!("{unreadable} could not be read");
    }
    if args.values.is_none() {
        // Locating is not reading, and the difference is worth one line.
        eprintln!("(locations only — add `--values` to read them)");
    }
    report_unreadable(&unparsed, &modes, 10);
    Ok(())
}

// ---------------------------------------------------------------------------
// properties
// ---------------------------------------------------------------------------

fn properties(args: PropertiesArgs, modes: Modes) -> Result<()> {
    use energyml_domain::property::{self, Discrepancy, PropertyKindRef};
    use energyml_domain::property_kind::{self, Dictionary, Resolver};

    let store = open(&args.file, &modes)?;

    // A dictionary the caller supplies replaces the built-in one for this run.
    // Loading it here rather than deep in the resolver means a bad path is one
    // clear error at the start instead of a thousand "kind not found".
    if let Some(path) = &args.dictionary {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the property kind dictionary {}", path.display()))?;
        let dictionary = Dictionary::from_xml(&text)
            .with_context(|| format!("loading {}", path.display()))?
            .labelled(format!("the dictionary loaded from {}", path.display()));
        eprintln!(
            "{} property kinds loaded from {}",
            dictionary.len(),
            path.display()
        );
        property_kind::set_default(dictionary);
    }
    let (dictionary_size, dictionary_label) = property_kind::builtin();
    if dictionary_size == 0 {
        eprintln!(
            "note: {dictionary_label} — kinds that are not objects of this package cannot be resolved; pass `--dictionary <file>` to load one"
        );
    }
    let resolver = Resolver::new(&store);

    let wanted = args.representation.as_deref().map(str::to_ascii_lowercase);

    let mut shown = 0usize;
    let mut unreadable = Vec::new();
    let mut refusals = 0usize;
    let mut disagreements = 0usize;
    let mut unresolved_kinds = 0usize;

    for summary in store.resources() {
        if !summary.qualified_type.ends_with("Property") {
            continue;
        }
        let object = match store.get_object(&summary.uri) {
            Ok(object) => object,
            Err(e) => {
                unreadable.push(format!("{}  {}  {e}", summary.uuid, summary.qualified_type));
                continue;
            }
        };
        let property = match property::read(object.as_reflect()) {
            Ok(property) => property,
            Err(e) => {
                unreadable.push(format!("{}  {}  {e}", summary.uuid, summary.qualified_type));
                continue;
            }
        };
        if let Some(wanted) = &wanted
            && property
                .supporting_representation
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_ref()
                != Some(wanted)
        {
            continue;
        }
        shown += 1;

        // What the document says about the kind, and — when it can be
        // followed — what it turns out to be.
        let (kind, resolution) = match &property.kind {
            Some(reference) => {
                let stated = match reference {
                    PropertyKindRef::Standard { name } => format!("kind {name} (standard)"),
                    PropertyKindRef::Object { title, uuid } => match title {
                        Some(title) => format!("kind {title} ({})", short_uuid(uuid)),
                        None => format!("kind {}", short_uuid(uuid)),
                    },
                };
                (stated, Some(resolver.resolve(reference)))
            }
            None => ("no kind".to_string(), None),
        };
        let counts = property
            .value_count_per_element
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("×");
        println!(
            "{}  {}  {}",
            property.uuid,
            property.qualified_type,
            property.title.as_deref().unwrap_or("<untitled>")
        );
        println!(
            "    on {} of {}  |  {kind}{}  |  {} value(s) per element  |  {} patch(es)",
            property.indexable,
            property
                .supporting_representation
                .as_deref()
                .map(short_uuid)
                .unwrap_or_else(|| "<nothing>".to_string()),
            property
                .uom
                .as_deref()
                .map(|u| format!("  |  uom {u}"))
                .unwrap_or_default(),
            if counts.is_empty() {
                "?".to_string()
            } else {
                counts
            },
            property.patches.len()
        );

        match resolution {
            Some(Ok(resolution)) => {
                let class = resolution
                    .quantity_class()
                    .map(|class| format!("  |  quantity class {class}"))
                    .unwrap_or_default();
                println!(
                    "    {} [{}]{class}",
                    resolution.path(),
                    resolution.kind().found_in
                );
                if resolution.uses_an_abstract_kind() {
                    // Common TRG §11.9 makes this an anomaly, not an error.
                    println!(
                        "    ⚠ `{}` is an abstract kind: it is meant to be a parent, not to be used directly",
                        resolution.kind().title
                    );
                }
                if let Some(date) = &resolution.kind().deprecated {
                    println!("    ⚠ `{}` was deprecated on {date}", resolution.kind().title);
                }
                for note in &resolution.notes {
                    println!("    · {note}");
                }
            }
            Some(Err(e)) => {
                unresolved_kinds += 1;
                println!("    kind not resolved: {e}");
            }
            None => println!("    ⚠ this property names no property kind, which the schema requires"),
        }

        // The count check, which is the reason this command exists rather than
        // being a `list --filter Property`.
        let Some(support) = property.supporting_representation.as_deref() else {
            continue;
        };
        let Ok(representation) = store.get_object(support) else {
            println!("    the supporting representation is not in this package");
            continue;
        };
        let actual = if args.check {
            let mut total = 0u64;
            let mut failed = None;
            for patch in &property.patches {
                match patch.read(&store) {
                    Ok(values) => total += values.len() as u64,
                    Err(e) => failed = Some(e),
                }
            }
            match failed {
                Some(e) => {
                    println!("    values not read: {e}");
                    None
                }
                None => Some(total),
            }
        } else {
            None
        };

        for discrepancy in property.check(representation.as_reflect(), actual) {
            match discrepancy {
                Discrepancy::CountUnknown { reason } => {
                    refusals += 1;
                    println!("    count not checked: {reason}");
                }
                other => {
                    disagreements += 1;
                    println!("    ⚠ {other}");
                }
            }
        }
    }

    println!("\n{shown} property(ies)");
    if args.check {
        println!("{disagreements} whose value count contradicts the document");
    } else {
        println!("(counts not verified against the values — add `--check` to read them)");
    }
    if refusals > 0 {
        println!("{refusals} whose element count this build cannot compute");
    }
    if unresolved_kinds > 0 {
        println!(
            "{unresolved_kinds} whose property kind is neither in the package nor in {dictionary_label}"
        );
    }
    report_unreadable(&unreadable, &modes, 10);
    Ok(())
}

/// First segment of a uuid — enough to recognise, short enough to read.
fn short_uuid(uuid: &str) -> String {
    uuid.split('-').next().unwrap_or(uuid).to_string()
}

// ---------------------------------------------------------------------------
// colors
// ---------------------------------------------------------------------------

fn colors(args: ColorsArgs, modes: Modes) -> Result<()> {
    use energyml_cache::colours::{self, Options, Wanted};
    use energyml_domain::color::{Level, Paint};

    let store = open(&args.file, &modes)?;
    let cache = layers::cache_for("colors");

    // **The decisions are the library's** (`energyml_cache::colours::of_package`,
    // the rule the front end's `colour_of` answers by): what is left here is the
    // printing. The set is one object of the package, and there may be none —
    // RESQML 2.0.1 defines no colour at all, so a 2.0.1 package having none is
    // the standard rather than a defect, and the message says which case it is.
    let report = colours::of_package(
        &cache,
        &store,
        &Wanted {
            object: args.object.as_deref(),
            options: Options {
                inherit: !args.no_inherit,
                viewer_kind: Some(args.viewer.clone()),
            },
        },
    )?;
    if report.sets == 0 {
        println!(
            "no `GraphicalInformationSet` in this package, so nothing states a colour.
             (RESQML 2.0.1 has no notion of colour at all — for a 2.0.1 model this is the              standard, not a gap.)"
        );
        return Ok(());
    }
    if report.sets > 1 {
        // Nothing forbids it and nothing says how they combine.
        println!(
            "{} graphical information sets; they are read in order",
            report.sets
        );
    }
    for note in &report.notes {
        eprintln!("note: {note}");
    }

    let mut answered = 0usize;
    let mut silent = 0usize;

    for row in &report.rows {
        let summary = &row.summary;
        // The citation, from the cheap header read rather than a full
        // deserialization: this command may walk every object of a package.
        let title = store
            .header(&summary.uri)
            .map(|header| header.titled())
            .unwrap_or_else(|_| "<untitled>".to_string());
        println!("{}  {}  {title}", summary.uuid, summary.qualified_type);
        match &row.decision {
            Some(decision) => {
                answered += 1;
                let origin = match decision.level {
                    Level::Property => "on the property itself".to_string(),
                    Level::PropertyKind => {
                        format!("on its property kind ({})", short_uuid(&decision.found_on))
                    }
                    Level::Representation => "on the representation".to_string(),
                    Level::Interpretation => {
                        format!(
                            "inherited from its interpretation ({})",
                            short_uuid(&decision.found_on)
                        )
                    }
                    Level::Feature => {
                        format!("inherited from its feature ({})", short_uuid(&decision.found_on))
                    }
                    Level::Collection => {
                        format!("inherited from a collection ({})", short_uuid(&decision.found_on))
                    }
                    Level::Model => format!("inherited from the model ({})", short_uuid(&decision.found_on)),
                };
                match &decision.paint {
                    Paint::Constant(rgba) => println!(
                        "    {} — {origin}, entry {}{}",
                        rgba.to_hex(),
                        decision.entry,
                        if decision.visible {
                            ""
                        } else {
                            "  [marked invisible]"
                        }
                    ),
                    Paint::Mapped {
                        map,
                        scaling,
                        value_vector_index,
                    } => {
                        let (low, high) = map.range();
                        println!(
                            "    {} map `{}` — {} stop(s) over {low}–{high} — {origin}, entry {}",
                            if map.continuous.is_some() {
                                "continuous"
                            } else {
                                "discrete"
                            },
                            map.title.as_deref().unwrap_or(&map.uuid),
                            map.stops.len(),
                            decision.entry
                        );
                        if let Some((min, max)) = scaling.min_max {
                            println!("      values clamped to {min}–{max} before mapping");
                        }
                        if let Some(component) = value_vector_index {
                            println!(
                                "      coloured by component {component} of the value vector \
                                 (an index into the data, not into the map)"
                            );
                        }
                    }
                }
                for note in &decision.notes {
                    println!("      · {note}");
                }
            }
            None => {
                silent += 1;
                println!("    no colour at any level — nothing is invented");
            }
        }
    }

    println!("\n{answered} object(s) with a colour, {silent} without");
    if args.no_inherit {
        println!("(inheritance off: only what each object carries itself)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// tables
// ---------------------------------------------------------------------------

/// `energyml wells` — the wellbores of a package, as a table.
///
/// The command **decides nothing**: `energyml_domain::wellbore_table` reads,
/// merges and serialises, and this one chooses the wellbores and prints. That
/// is the repository rule, and it has a concrete consequence here: the front end's
/// wells view renders exactly this file, because it calls the same functions.
fn wells(args: WellsArgs, modes: Modes) -> Result<()> {
    use energyml_domain::wellbore_table as wt;

    // **Everything is in `energyml_geometry::wells`**: the properties arranged once,
    // what surrounds each frame, the headers decided for the file.
    // The front end and Python call the same function.
    let store = open(&args.file, &modes)?;
    let export = energyml_geometry::wells::of_store(&store.resources(), &store, &args.wells);
    for note in &export.notes {
        eprintln!("  [left out] {note}");
    }
    let tables = export.tables;
    if tables.is_empty() {
        bail!("no wellbore frame of this package matches");
    }

    // **One file per wellbore**: each is merged alone, so no conflict is
    // possible — the output a column disagreement suggests.
    if args.separate {
        let dir = args.out.clone().unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        for table in &tables {
            let name = table.well.replace(['/', '\\', ':'], "_");
            let path = dir.join(format!("{name}.csv"));
            let mut one = wt::merge(std::slice::from_ref(table));
            if let Some(first) = one.headers.first_mut() {
                *first = "well name".to_string();
            }
            let text = wt::to_csv(&one, args.separator);
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        }
        println!("{} file(s) written to {}", tables.len(), dir.display());
        return Ok(());
    }

    let merged = export.merged;
    if !merged.conflicts.is_empty() && !args.force {
        for clash in &merged.conflicts {
            eprintln!("  [clash] {}", clash.says());
        }
        bail!(
            "{} column(s) disagree between these wellbores, so one file would put two measurements under one heading.
                    Use `--separate` for one file each, or `--force` to write anyway",
            merged.conflicts.len()
        );
    }
    for clash in &merged.conflicts {
        eprintln!("  [clash, written anyway] {}", clash.says());
    }
    let text = wt::to_csv(&merged, args.separator);
    match &args.out {
        Some(path) => {
            std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
            println!(
                "{} wellbore(s), {} row(s), {} column(s) written to {}",
                tables.len(),
                merged.rows.len(),
                merged.headers.len(),
                path.display()
            );
        }
        None => print!("{text}"),
    }
    Ok(())
}

fn tables(args: TablesArgs, modes: Modes) -> Result<()> {
    use energyml_domain::table::{Lookup, Table};

    let store = open(&args.file, &modes)?;
    let mut found = 0usize;

    for summary in store.resources() {
        let ty = &summary.qualified_type;
        if !(ty.ends_with("ColumnBasedTable")
            || ty.ends_with("StringTableLookup")
            || ty.ends_with("DoubleTableLookup"))
        {
            continue;
        }
        let object = match store.get_object(&summary.uri) {
            Ok(object) => object,
            Err(e) => {
                eprintln!("{}: {e}", summary.uuid);
                continue;
            }
        };
        let table = match Table::read(object.as_reflect()) {
            Ok(table) => table,
            Err(e) => {
                eprintln!("{}: {e}", summary.uuid);
                continue;
            }
        };
        found += 1;

        println!(
            "{}  {}  {}",
            table.uuid,
            table.qualified_type,
            table.title.as_deref().unwrap_or("<untitled>")
        );
        for column in &table.columns {
            let kind = match &column.kind {
                Some(energyml_domain::property::PropertyKindRef::Object { title, uuid }) => {
                    format!("  kind {}", title.as_deref().unwrap_or(uuid))
                }
                Some(energyml_domain::property::PropertyKindRef::Standard { name }) => {
                    format!("  kind {name}")
                }
                None => String::new(),
            };
            println!(
                "    {}{}{}{}{}",
                if column.key { "[key] " } else { "      " },
                column.title.as_deref().unwrap_or("<untitled>"),
                kind,
                column
                    .uom
                    .as_deref()
                    .map(|uom| format!("  uom {uom}"))
                    .unwrap_or_default(),
                if column.values_per_row > 1 {
                    format!("  {} values per row", column.values_per_row)
                } else {
                    String::new()
                }
            );
        }

        // The values, which for a column-based table means reading the file.
        match Lookup::read(object.as_reflect(), &store) {
            Ok(lookup) => {
                println!("    {} row(s)", lookup.rows.rows);
                for note in &lookup.notes {
                    println!("    ⚠ {note}");
                }
                for (key, rows) in lookup.duplicate_keys() {
                    println!(
                        "    ⚠ the key ({}) is on rows {:?}; a lookup answers with the first, so \
                         the others are unreachable",
                        key.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "),
                        rows
                    );
                }
                for row in 0..lookup.rows.rows.min(args.rows) {
                    let cells: Vec<String> = lookup
                        .rows
                        .row(row)
                        .into_iter()
                        .map(|cell| cell.map(|c| c.to_string()).unwrap_or_else(|| "—".to_string()))
                        .collect();
                    println!("      {}", cells.join("  "));
                }
                if lookup.rows.rows > args.rows {
                    println!("      … and {} more row(s)", lookup.rows.rows - args.rows);
                }
            }
            Err(e) => println!("    values not read: {e}"),
        }
    }

    if found == 0 {
        println!("no lookup table in this package");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// geometry
// ---------------------------------------------------------------------------

fn geometry(args: GeometryArgs, modes: Modes) -> Result<()> {
    use energyml_geometry::{Topology, VerticalQuantity, scene};

    let store = open(&args.file, &modes)?;
    let mut unconverted: Vec<String> = Vec::new();

    // **The same reading `export` does**, and written once (`scene::of_package`).
    // It was two loops that had begun to differ; what is left here is the
    // printing, which is what a command is for.
    let resources = store.resources();
    let cache = layers::cache_for("geometry");
    let colouring = energyml_cache::colours::Colouring {
        cache: &cache,
        store: &store,
    };
    let read = scene::of_package(
        &resources,
        &store,
        &scene::Request {
            of_type: args.filter.as_deref(),
            colours: Some(&colouring),
            ..Default::default()
        },
    );
    let (drawn, points) = (
        read.objects.len(),
        read.objects
            .iter()
            .map(|o| o.geometry.point_count())
            .sum::<usize>(),
    );
    let (refused, unparsed) = (read.refused, read.unreadable);

    for object in &read.objects {
        let placement = &object.crs;
        let shape = &object.geometry;

        // Say the unit too. A CRS in feet is an ordinary file, and reading it
        // as metres puts the model 3.28 times too far out with every
        // coordinate still looking plausible.
        let where_it_is = match &placement {
            Some(crs) => {
                let system = match crs.projected.epsg() {
                    Some(code) => format!("EPSG:{code}"),
                    None => format!("{:?}", crs.projected),
                };
                let unit = crs.horizontal_uom.as_deref().unwrap_or("unstated");
                let time = if crs.vertical_quantity == VerticalQuantity::Time {
                    ", time"
                } else {
                    ""
                };
                format!("{system}, {unit}{time}")
            }
            None => "no CRS".to_string(),
        };
        println!(
            "{}  {}  {} patch(es), {} point(s)  [{where_it_is}]",
            object.uuid,
            object.qualified_type,
            shape.patches.len(),
            shape.point_count()
        );

        // Said out loud, because it is the difference between what the file
        // holds and what an export will contain. A node whose coordinates are
        // NaN is a null the standard defines and a position no format can
        // write, so the exporters drop it — a count that only showed up as a
        // smaller output file would be a surprise. See `energyml_geometry::defined`.
        let undefined = shape.undefined_count();
        if undefined > 0 {
            println!("    {undefined} node(s) with no position — counted here, dropped on export");
        }

        for patch in &shape.patches {
            let what = match &patch.topology {
                Topology::Points => "points".to_string(),
                Topology::Triangles(t) => format!("{} triangle(s)", t.len()),
                Topology::Quads(q) => format!("{} quadrilateral(s)", q.len()),
                Topology::Polygons(r) => format!("{} polygon(s)", r.len()),
                Topology::Polylines(runs) => format!("{} polyline(s)", runs.len()),
                Topology::Segments(pairs) => format!("{} edge segment(s)", pairs.len()),
                Topology::Grid { fastest, slowest } => format!("grid {fastest}×{slowest}"),
                Topology::Hexahedra {
                    cells, dimensions, ..
                } => format!(
                    "{} cell(s), {}×{}×{}",
                    cells.len(),
                    dimensions[0],
                    dimensions[1],
                    dimensions[2]
                ),
                // The gap between the face count and the cell-face pairs is
                // the number of shared faces, and it is worth seeing: a grid
                // whose cells share nothing is a heap of loose boxes.
                Topology::Polyhedra(cells) => format!(
                    "{} cell(s), {} face(s), {} cell-face pair(s)",
                    cells.cell_count(),
                    cells.face_count(),
                    cells.slot_count()
                ),
            };
            println!(
                "    patch {} @{}  {} point(s)  {what}",
                patch.index,
                patch.node_offset,
                patch.points.len()
            );
            let Some(limit) = args.points else { continue };
            for point in patch.points.iter().take(limit) {
                let placed = match (placement, args.projected, args.metres) {
                    // Normalising the units is a separate request from placing
                    // the point, and it can fail where placing cannot.
                    (Some(crs), _, true) => match crs.to_metres(*point) {
                        Ok(metric) => metric,
                        Err(e) => {
                            unconverted.push(format!("{}  {e}", object.uuid));
                            continue;
                        }
                    },
                    (Some(crs), true, false) => crs.to_projected(*point),
                    _ => *point,
                };
                println!("      {:.3}  {:.3}  {:.3}", placed[0], placed[1], placed[2]);
            }
        }
    }

    eprintln!();
    eprintln!("{drawn} representation(s) evaluated, {points} point(s)");
    // Never silent: a form this build cannot evaluate is named, because empty
    // geometry and unsupported geometry look identical from the outside.
    if !refused.is_empty() {
        eprintln!("{} not evaluated:", refused.len());
        for line in &refused {
            eprintln!("  {line}");
        }
    }
    if !unconverted.is_empty() {
        eprintln!("{} not converted to metres:", unconverted.len());
        for line in &unconverted {
            eprintln!("  {line}");
        }
    }
    if args.projected || args.metres {
        eprintln!("(local placement applied; no reprojection between EPSG systems)");
    }
    report_unreadable(&unparsed, &modes, 10);
    Ok(())
}

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

fn export(args: ExportArgs, modes: Modes) -> Result<()> {
    use energyml_export::{Exporters, Options};
    use energyml_geometry::{scene, transfer};

    let exporters = Exporters::builtin();
    if args.list {
        for exporter in exporters.all() {
            println!(
                "{:<10} .{:<9} {:<22} {}",
                exporter.name(),
                exporter.extension(),
                exporter.mime(),
                exporter.describe()
            );
        }
        return Ok(());
    }
    let exporter = exporters.get(&args.format)?;

    let spec = match (&args.metadata, args.standard_metadata) {
        (Some(path), _) => {
            let text =
                std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            Some(energyml_domain::metadata::Spec::from_json(&text)?)
        }
        (None, true) => Some(energyml_domain::metadata::Spec::standard()),
        (None, false) => None,
    };

    let store = open(&args.file, &modes)?;

    // Where to look at the values, which is not where the file hangs them.
    let onto = match args.properties_on.as_deref() {
        Some("points") => Some(transfer::Scope::Points),
        Some("cells") => Some(transfer::Scope::Cells),
        _ => None,
    };
    // Parsed before anything is read, so a typo fails on the command line.
    let method = transfer::Method::parse(&args.transfer_method)?;
    let wanted: Vec<String> = args.property.iter().map(|u| u.to_ascii_lowercase()).collect();

    // **The reading is the library's** (`scene::of_package`): the walk, the
    // shapes, the properties and the colour ramps. It was written here and in
    // `geometry`, and the two had begun to differ.
    let resources = store.resources();
    // The colour ramps are the front end's: the same rule, by the cache the command's
    // fixed layers (`layers.rs`) turn on.
    let cache = layers::cache_for("export");
    let colouring = energyml_cache::colours::Colouring {
        cache: &cache,
        store: &store,
    };
    let read = scene::of_package(
        &resources,
        &store,
        &scene::Request {
            of_type: args.filter.as_deref(),
            colours: Some(&colouring),
            spec: spec.as_ref(),
            properties: match (args.properties, wanted.is_empty()) {
                (_, false) => scene::Properties::Only(&wanted),
                (true, true) => scene::Properties::All,
                (false, true) => scene::Properties::None,
            },
            onto,
            method: Some(method),
            ..Default::default()
        },
    );
    let mut objects = read.objects;
    let refused = read.refused;
    let unparsed = read.unreadable;
    let unattached = read.unattached;

    // The geodesy backend. Built even when nothing asks for one: GeoJSON needs
    // it whether or not `--epsg` was given, and building it costs a `BTreeMap`.
    let mut backend = energyml_proj::Proj4::new();
    if let Some(path) = &args.crs_definitions {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        backend = backend.with_definitions_json(&text)?;
    }
    if args.allow_silent_datum {
        backend = backend.allowing_a_silent_datum();
    }

    // The tube, last of the geometry decisions: it is a *rendering* of the
    // lines a shape settled on, so sweeping before that would sweep the wrong
    // thing. Refused by its number here rather than per object, because a
    // radius of zero is wrong for every one of them.
    if let Some(radius) = args.tube {
        let tube = energyml_geometry::tube::Tube::checked(radius, args.tube_sides)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut swept = 0usize;
        for object in &mut objects {
            // An object with no polylines is left as it is: `--tube` on a scene
            // of wells and horizons must sweep the wells, not refuse the export.
            if let Ok(solid) = energyml_geometry::tube::sweep_object(object, &tube) {
                *object = solid;
                swept += 1;
            }
        }
        eprintln!(
            "{swept} object(s) swept into a tube of radius {radius} with {} sides",
            args.tube_sides
        );
    }

    let mut options = Options::new().with_reprojector(&backend);
    if let Some(code) = args.epsg {
        options = options.with_target_epsg(code);
    }
    if args.native_crs {
        options = options.with_native_crs();
    }
    // Parsed before anything is written, so a typo in a rule fails on the
    // command line rather than after twenty files have landed in a directory.
    let mut shapes = energyml_export::Shapes::new();
    for rule in &args.shapes {
        shapes = shapes.parse_rule(rule).map_err(anyhow::Error::msg)?;
    }
    options = options.with_shapes(shapes);
    if let Some(per_second) = args.time_equivalence {
        options = options.with_time_equivalence(per_second);
    }

    // The message, on the terminal, whether or not the flag was given — because
    // the two things a person needs to know are opposite sides of the same
    // fact: without the flag those objects are silently absent from the file,
    // and with it their Z is a number somebody chose.
    let in_time = objects
        .iter()
        .filter(|o| o.crs.as_ref().is_some_and(|crs| crs.is_time()))
        .count();
    if in_time > 0 {
        match args.time_equivalence {
            Some(per_second) => {
                eprintln!(
                    "{in_time} representation(s) have a vertical axis in travel time; written with 1 s = {per_second} m."
                );
                eprintln!("  An equivalence, not a depth: the package carries no velocity model, and");
                eprintln!("  RESQML does not say whether the time is one-way or two-way, so a real");
                eprintln!("  depth is half or double this. The number is written into each file.");
            }
            None => {
                eprintln!("{in_time} representation(s) have a vertical axis in travel time and are skipped:");
                eprintln!("  they cannot be placed in metres. Pass --time-equivalence <M_PER_S> to");
                eprintln!("  write them anyway; 2000 is a mid-range sediment velocity.");
                // Named, one per line: a count says how many are missing from the
                // folder, never which — and "which" is what somebody opening the
                // folder asks first.
                for o in objects
                    .iter()
                    .filter(|o| o.crs.as_ref().is_some_and(|crs| crs.is_time()))
                {
                    eprintln!("  skipped: {} {} ({})", o.uuid, o.qualified_type, o.title);
                }
            }
        }
    }

    let writable: Vec<_> = objects
        .iter()
        .filter(|o| exporter.handles_with(o, &options))
        .cloned()
        .collect();
    let skipped = objects.len() - writable.len();

    let mut written = 0usize;
    // Objects the exporter itself refused, one line each. Separate from
    // `refused` — that one is the geometry saying it cannot evaluate; this is
    // the writing saying it cannot place, convert or encode what was evaluated.
    let mut unwritable: Vec<String> = Vec::new();
    match &args.out {
        // A directory: one file per representation, named
        // `qualifiedType_uuid.ext`.
        //
        // The uuid is what makes it unique — two objects share a title far more
        // often than anyone expects, and a folder of `Horizon.vtu` overwriting
        // itself is a silent loss. The qualified type in front is what makes it
        // *readable*: a folder of thirty uuids says nothing, and the one thing a
        // person wants to know before opening a file is what kind of thing it
        // is. It carries the release too (`resqml20.` against `resqml22.`),
        // which matters because a package may hold both — this one does.
        Some(path) if path.is_dir() || path.extension().is_none() => {
            std::fs::create_dir_all(path)?;
            for object in &writable {
                let name = format!(
                    "{}_{}.{}",
                    file_safe(&qualified_type_of(object)),
                    object.uuid,
                    exporter.extension()
                );
                let target = path.join(&name);
                let file = std::fs::File::create(&target)?;
                let mut out = std::io::BufWriter::new(file);
                // **One object failing is not the run failing.** A directory
                // export is a batch, and a batch that stops at the first
                // refusal leaves the caller with a folder that looks like a
                // finished export and holds a fraction of one — which is the
                // failure this whole workspace is arranged against. A time-based
                // CRS reaching a metre conversion is exactly that case: one
                // object of thirty-three, and it used to take the other
                // thirty-two with it.
                match exporter.write(object, &mut out, &options) {
                    Ok(()) => written += 1,
                    Err(e) => {
                        // And take the stub with it. A zero-byte `.vtk` sitting
                        // in the output is worse than no file: it reads as a
                        // representation with nothing in it.
                        drop(out);
                        let _ = std::fs::remove_file(&target);
                        unwritable.push(format!("{}  {}  {e}", object.uuid, object.qualified_type));
                    }
                }
            }
        }
        Some(path) => {
            let file = std::fs::File::create(path)?;
            let mut out = std::io::BufWriter::new(file);
            exporter.write_all(&writable, &mut out, &options)?;
            written = writable.len();
        }
        None => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            exporter.write_all(&writable, &mut out, &options)?;
            written = writable.len();
        }
    }

    eprintln!();
    eprintln!("{written} representation(s) written as {}", exporter.name());
    // A spec key that matched **nowhere** is almost always a typo, and it is
    // invisible in the output — the file simply lacks a property.
    //
    // Only nowhere: a key that found a value on some objects and not others is
    // ordinary. Half the surfaces in a package name an interpretation and half
    // do not, and reporting that as a problem would train people to ignore the
    // line that matters.
    if let Some(spec) = &spec {
        let mut matched: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for object in &writable {
            for (key, _) in object.metadata.pairs() {
                matched.insert(key);
            }
        }
        let never: Vec<&str> = spec
            .entries()
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|key| !matched.contains(key))
            .collect();
        if !never.is_empty() {
            // With the path, not just the key. A key that matches nothing on
            // every object of a package is almost never an absent value — it is
            // a path that does not exist in this release, and seeing
            // `RepresentedInterpretation.uuid` beside the key is what tells a
            // reader that resqml 2.2 spells that element `RepresentedObject`.
            eprintln!("metadata key(s) that matched nothing anywhere:");
            for key in &never {
                match spec.get_path(key) {
                    Some(path) => eprintln!("  {key}  ({path})"),
                    None => eprintln!("  {key}"),
                }
            }
            eprintln!("  (written as null in geojson; omitted from obj and vtk)");
        }
    }
    // Never silent about what did not make it into the file: a partial export
    // that looks complete is the failure this whole workspace is arranged
    // against.
    if skipped > 0 {
        eprintln!("{skipped} skipped: `{}` cannot write them", exporter.name());
        if exporter.name() == "geojson" && !args.native_crs {
            // The most common reason, and it has an exit: no usable EPSG code.
            eprintln!(
                "  (objects without a usable EPSG code are skipped; \
                 `--native-crs` writes them in their stored coordinates)"
            );
            // And *which* CRS, because "no usable EPSG code" over a package
            // that plainly contains one sends the reader looking at the
            // exporter instead of at the CRS chain. An eml 2.3 CRS is three
            // objects, and the code lives in the third: a compound CRS whose
            // `LocalEngineering2dCrs` is not in the package resolves to a CRS
            // that says "unknown system", and nothing else complains.
            let mut named: Vec<String> = objects
                .iter()
                .filter(|o| !exporter.handles_with(o, &options))
                .filter_map(|o| o.crs.as_ref())
                .filter(|crs| crs.projected.epsg().is_none())
                .map(|crs| format!("{} ({})", crs.uuid, crs.kind))
                .collect();
            named.sort();
            named.dedup();
            if !named.is_empty() {
                eprintln!("  the CRS they name gives no EPSG code:");
                for line in named.iter().take(5) {
                    eprintln!("    {line}");
                }
                if named.len() > 5 {
                    eprintln!("    … and {} more", named.len() - 5);
                }
                eprintln!("    `energyml check <file>` lists references that point at nothing");
            }
        }
    }
    if args.native_crs && written > 0 {
        // Loud on purpose: the file that just came out is not RFC 7946
        // conformant, and nothing inside it will error anywhere.
        eprintln!(
            "coordinates written as stored, NOT reprojected to WGS 84 (--native-crs); \
             the file carries a legacy `crs` member when the EPSG code is known"
        );
    }
    if !unattached.is_empty() {
        // Named, never silent: a property missing from an export leaves a
        // picture that looks finished and is not.
        eprintln!("{} property attachment(s) refused:", unattached.len());
        for line in unattached.iter().take(10) {
            eprintln!("  {line}");
        }
        if unattached.len() > 10 {
            eprintln!("  … and {} more", unattached.len() - 10);
        }
    }
    if !unwritable.is_empty() {
        eprintln!(
            "{} evaluated but could not be written as {}:",
            unwritable.len(),
            exporter.name()
        );
        for line in unwritable.iter().take(10) {
            eprintln!("  {line}");
        }
        if unwritable.len() > 10 {
            eprintln!("  … and {} more", unwritable.len() - 10);
        }
    }
    if !refused.is_empty() {
        eprintln!("{} could not be evaluated:", refused.len());
        for line in &refused {
            eprintln!("  {line}");
        }
    }
    report_unreadable(&unparsed, &modes, 10);
    Ok(())
}

/// Shape, element type, and the first few numbers.
fn preview(data: &energyml_storage::ArrayData, limit: usize) -> String {
    let shape = data
        .metadata
        .dimensions
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("×");
    let values = data.to_f64();
    let shown = values
        .iter()
        .take(limit)
        .map(|v| format!("{v}"))
        .collect::<Vec<_>>()
        .join(", ");
    let head = if values.len() > limit {
        format!("[{shown}, … {} more]", values.len() - limit)
    } else {
        format!("[{shown}]")
    };
    format!("{shape} {} {head}", data.metadata.element_type)
}

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

/// The model's documentation as JSON — the serialization is
/// `energyml_core::docs`; what lives here is choosing files and writing them.
///
/// The point is a **document**: the same facts the documentation view shows,
/// fetchable without running the application — and without the Energistics
/// login the schema downloads sit behind. Deployed beside the front end, each
/// release is one URL.
fn docs(args: DocsArgs) -> Result<()> {
    let registry = registry::all();
    let releases = energyml_core::docs::registry_docs(&registry);
    let wanted: Vec<serde_json::Value> = releases
        .as_array()
        .into_iter()
        .flatten()
        .filter(|release| {
            args.package
                .as_deref()
                .is_none_or(|package| release["package"] == package)
                && args
                    .version
                    .as_deref()
                    .is_none_or(|version| release["version"] == version)
        })
        .cloned()
        .collect();
    if wanted.is_empty() {
        // Named, with what exists: "no output" from a filter typo reads as an
        // empty model.
        let known: Vec<String> = releases
            .as_array()
            .into_iter()
            .flatten()
            .map(|release| {
                format!(
                    "{}/{}",
                    release["package"].as_str().unwrap_or("?"),
                    release["version"].as_str().unwrap_or("?")
                )
            })
            .collect();
        anyhow::bail!(
            "no registered release matches; this build holds: {}",
            known.join(", ")
        );
    }

    let Some(dir) = args.out else {
        println!("{}", serde_json::Value::Array(wanted));
        return Ok(());
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut index = Vec::new();
    for release in &wanted {
        let package = release["package"].as_str().unwrap_or("unknown");
        let version = release["version"].as_str().unwrap_or("unknown");
        let file = format!("{package}-{version}.json");
        std::fs::write(dir.join(&file), release.to_string()).with_context(|| format!("writing {file}"))?;
        index.push(serde_json::json!({
            "package": package,
            "version": version,
            "schemaVersion": release["schemaVersion"],
            "types": release["types"].as_array().map(Vec::len).unwrap_or(0),
            "file": file,
        }));
        println!(
            "{file}: {} type(s)",
            release["types"].as_array().map(Vec::len).unwrap_or(0)
        );
    }
    std::fs::write(
        dir.join("index.json"),
        serde_json::Value::Array(index).to_string(),
    )
    .context("writing index.json")?;

    // A browser pointed at `/docs/` gets this rather than a 404 or — served
    // under a single-page application — the application itself, which reads as
    // "the docs are not there" to anyone who tries the obvious address. Plain
    // links, no styling to maintain: the JSON files are the interface, this is
    // the signpost.
    let mut listing = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>EnergyML model documentation</title>\
         <h1>EnergyML model documentation</h1>\
         <p>One JSON per (package, version) — every type, field and declared value, \
         with the <code>xsd:documentation</code>. <a href=\"index.json\">index.json</a> \
         is the machine-readable list.</p><ul>",
    );
    for release in &wanted {
        let package = release["package"].as_str().unwrap_or("unknown");
        let version = release["version"].as_str().unwrap_or("unknown");
        let count = release["types"].as_array().map(Vec::len).unwrap_or(0);
        listing.push_str(&format!(
            "<li><a href=\"{package}-{version}.json\">{package} {version}</a> — {count} types</li>"
        ));
    }
    listing.push_str("</ul>");
    std::fs::write(dir.join("index.html"), listing).context("writing index.html")?;
    Ok(())
}

fn types(args: TypesArgs) -> Result<()> {
    let registry: Registry = registry::all();
    if let Some(wanted) = &args.describe {
        return describe(&registry, wanted);
    }
    let package = args.package.map(|p| p.to_ascii_lowercase());
    let mut count = 0usize;
    let mut all = 0usize;

    for info in registry.top_level_types() {
        all += 1;
        if let Some(package) = &package
            && !info.package.eq_ignore_ascii_case(package)
        {
            continue;
        }
        count += 1;
        let nestable = if info.is_nestable {
            "  (may also be nested)"
        } else {
            ""
        };
        println!("{:<44}{nestable}", info.qualified_type.unwrap_or(info.name));
    }

    eprintln!();
    eprintln!("{count} top-level type(s) shown, {all} registered");
    Ok(())
}

/// What may go in each field of a type — the schema's own answer, read back
/// out of the generated reflection.
fn describe(registry: &Registry, wanted: &str) -> Result<()> {
    use energyml_core::reflect::{TypeInfo, TypeKind};

    let (type_name, field_name) = match wanted.split_once('.') {
        // `resqml22.Foo` is a qualified type, not a field selector: the
        // segment before the dot is a domain, and no type is called `resqml22`.
        Some((left, right)) if registry.by_qualified_type(wanted).is_some() || left.is_empty() => {
            let _ = right;
            (wanted, None)
        }
        Some((left, right)) => (left, Some(right)),
        None => (wanted, None),
    };

    let info: &'static TypeInfo = registry
        .by_qualified_type(type_name)
        .map(|entry| entry.info)
        // A nested type has no qualified type — `Citation`, `TrianglePatch`,
        // every enumeration — and enumerations are not registered at all,
        // since nothing is ever a *part* of that type. They are still
        // reachable: every one of them is the type of some field of something
        // registered, so the search walks out from the registry.
        .or_else(|| registry.find_type(type_name))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no type named `{type_name}` in this build; `energyml types` lists the top-level \
                 ones, and a nested type is reachable by its plain name"
            )
        })?;

    println!("{} ({}{})", info.name, info.package, info.version);
    if let Some(qualified) = info.qualified_type {
        println!("  qualified type {qualified}");
    }
    if !info.doc.is_empty() {
        println!("  {}", info.doc.split('\n').next().unwrap_or_default().trim());
    }

    match info.kind {
        // An enumeration: the values, and whether a free string is allowed
        // beside them. That second half is the whole point of the `…Ext`
        // types, and nothing in the value itself says which kind it is.
        TypeKind::Enumeration => {
            println!(
                "\n  {} enumeration, {} declared value(s){}",
                if info.is_open { "open" } else { "closed" },
                info.variants.len(),
                if info.is_open {
                    " — and any other string, kept verbatim in `Other`"
                } else {
                    " — and nothing else"
                }
            );
            for variant in info.variants {
                println!("    {}", variant.value);
            }
        }
        // A choice: the concrete types a document may put here, each with the
        // `xsi:type` that selects it.
        TypeKind::Choice => {
            println!("\n  a choice of {} type(s):", info.variants.len());
            for variant in info.variants {
                println!(
                    "    {:<44}{}",
                    variant.value,
                    variant
                        .xsi_type
                        .map(|t| format!("xsi:type=\"{t}\""))
                        .unwrap_or_default()
                );
            }
        }
        TypeKind::Struct => {
            let fields: Vec<_> = match field_name {
                Some(name) => info
                    .fields
                    .iter()
                    .filter(|f| f.name.eq_ignore_ascii_case(name) || f.xml_name.eq_ignore_ascii_case(name))
                    .collect(),
                None => info.fields.iter().collect(),
            };
            if fields.is_empty() {
                anyhow::bail!(
                    "`{}` has no field named `{}`",
                    info.name,
                    field_name.unwrap_or_default()
                );
            }
            println!("\n  {} field(s):", fields.len());
            for field in fields {
                let cardinality = match field.occurs.max {
                    energyml_core::reflect::MaxOccurs::Bounded(max) => {
                        format!("{}..{max}", field.occurs.min)
                    }
                    energyml_core::reflect::MaxOccurs::Unbounded => format!("{}..∞", field.occurs.min),
                };
                println!(
                    "    {:<32} {:<8} {}{}",
                    field.xml_name,
                    cardinality,
                    match field.binding {
                        energyml_core::reflect::Binding::Attribute => "attribute",
                        energyml_core::reflect::Binding::Element => "element",
                        energyml_core::reflect::Binding::Text => "text content",
                        _ => "any",
                    },
                    if field.is_list { "  (xsd:list)" } else { "" }
                );

                // The values, for an enumeration.
                let values = field.ty.allowed_values();
                if !values.is_empty() {
                    let type_info = field.ty.type_info().expect("an enumeration has a type");
                    let shown: Vec<&str> = values.iter().map(|v| v.value).take(12).collect();
                    println!(
                        "      {} `{}`: {}{}",
                        if type_info.is_open {
                            "open enumeration"
                        } else {
                            "closed enumeration"
                        },
                        type_info.name,
                        shown.join(", "),
                        if values.len() > shown.len() {
                            format!(", … ({} values)", values.len())
                        } else {
                            String::new()
                        }
                    );
                    if type_info.is_open {
                        println!("      …or any other string, kept verbatim");
                    }
                    continue;
                }

                // The types, for anything else that is not a scalar.
                let accepted = field.accepted_types();
                match accepted.len() {
                    0 => {}
                    1 if !accepted[0].is_object_ref => println!("      {}", accepted[0].name),
                    _ => {
                        let names: Vec<&str> = accepted.iter().map(|t| t.name).take(12).collect();
                        println!(
                            "      one of: {}{}",
                            names.join(", "),
                            if accepted.len() > names.len() {
                                format!(", … ({} types)", accepted.len())
                            } else {
                                String::new()
                            }
                        );
                    }
                }
                if accepted.len() == 1 && accepted[0].is_object_ref {
                    // The one thing reflection cannot answer, and saying so is
                    // better than leaving a caller to assume.
                    println!(
                        "      a reference to another object — the XSD does not declare what it \
                         may point at; the RESQML technical reference guide gives the associations"
                    );
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// repair
// ---------------------------------------------------------------------------

/// The last resort, and the only one for a part no parser will touch.
///
/// Every other command here reads objects. This one reads **bytes**, because
/// the parts it exists for have none: markup that does not close, a byte that
/// is not UTF-8, a part of zero length. There is nothing to deserialise and
/// nothing to edit by reflection, so the loop is the one a text editor gives —
/// take the bytes out, fix them, put them back:
///
/// ```text
/// energyml repair model.epc                          # what is broken, and why
/// energyml repair model.epc --extract broken/        # the bytes, under their part names
/// $EDITOR broken/TriangulatedSetRepresentation_….xml
/// energyml repair model.epc --apply broken/ --out fixed.epc
/// ```
///
/// `--apply` says, for each part, whether the bytes now parse and as what, and
/// refuses to stage one that still does not unless `--force` says otherwise.
fn repair(args: RepairArgs, modes: Modes) -> Result<ExitCode> {
    let mut store = open(&args.file, &modes)?;
    let broken = store.unreadable();

    if args.extract.is_none() && args.apply.is_none() {
        if broken.is_empty() {
            println!("every part of {} reads as an object", args.file.display());
            return Ok(ExitCode::SUCCESS);
        }
        println!("{} unreadable part(s) in {}:", broken.len(), args.file.display());
        for item in &broken {
            println!("  {}  ({} byte(s))", item.part, item.bytes);
            // The absence of a uri is the worse diagnosis of the two, so it is
            // said rather than left to be inferred from a missing field.
            match &item.uri {
                Some(uri) => println!("    {uri}"),
                None => println!("    no uri: nothing in the inventory can address it"),
            }
            if let Some(content_type) = &item.content_type {
                println!("    declared {content_type}");
            }
            println!("    {}", item.reason);
        }
        println!("\n`--extract <dir>` writes their bytes out; `--apply <dir>` puts them back");
        return Ok(ExitCode::FAILURE);
    }

    if let Some(directory) = &args.extract {
        for item in &broken {
            let target = directory.join(&item.part);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            let bytes = store.raw_part(&item.part)?;
            std::fs::write(&target, &bytes).with_context(|| format!("writing {}", target.display()))?;
            println!("{} → {} ({} byte(s))", item.part, target.display(), bytes.len());
        }
        if broken.is_empty() {
            println!("nothing to extract: every part reads as an object");
        }
    }

    let Some(directory) = &args.apply else {
        return Ok(ExitCode::SUCCESS);
    };

    let mut staged = 0usize;
    let mut refused = 0usize;
    for (part, bytes) in collect_parts(directory, directory)? {
        // Through `stage_repaired_part` rather than `set_raw_part`: bytes that
        // parse become an **object**, so the save rebuilds this part's `.rels`
        // from its own references. Bytes staged as bytes keep whatever
        // relationships they had, which for a part that was broken is often
        // none.
        match store.stage_repaired_part(&part, bytes.clone()) {
            Ok(kind) => {
                println!("{part}: now reads as {kind}");
                staged += 1;
            }
            Err(e) if args.force => {
                store.set_raw_part(&part, bytes);
                println!("{part}: still unreadable, staged anyway — {e}");
                staged += 1;
            }
            Err(e) => {
                println!("{part}: still unreadable, not staged — {e}");
                refused += 1;
            }
        }
    }

    let Some(out) = &args.out else {
        println!("\n{staged} part(s) would be staged, {refused} refused; `--out <file>` writes them");
        return Ok(if refused > 0 {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        });
    };
    let diagnostics = store.save_as(out, &energyml_storage::SaveOptions::default())?;
    println!(
        "\nwrote {} — {staged} part(s) repaired, {refused} refused",
        out.display()
    );
    for diagnostic in diagnostics.iter().take(10) {
        println!(
            "  [{:?}] {}: {}",
            diagnostic.severity, diagnostic.part, diagnostic.message
        );
    }
    Ok(if refused > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Every file under a directory, keyed by its path relative to the root.
///
/// The part name **is** the relative path, which is what makes `--extract` and
/// `--apply` inverses of one another: a part called
/// `docProps/whatever.xml` comes out in a `docProps/` folder and goes back
/// from one.
fn collect_parts(root: &PathBuf, directory: &PathBuf) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(directory).with_context(|| format!("reading {}", directory.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(collect_parts(root, &path)?);
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((name, std::fs::read(&path)?));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

// ---------------------------------------------------------------------------
// graph
// ---------------------------------------------------------------------------

/// The reference graph, in a shape something else can draw.
///
/// The nodes carry what a viewer labels with — uri, uuid, type, concept, title
/// — and the objects the package *references without holding* are in there too,
/// marked `present: false`. A graph that quietly dropped those would draw a
/// self-contained package where there is none, which is the one thing a graph
/// of references is for noticing.
fn graph_command(args: GraphArgs, modes: Modes) -> Result<()> {
    let store = open(&args.file, &modes)?;
    let summaries: Vec<_> = store
        .resources()
        .into_iter()
        .filter(|summary| match &args.filter {
            Some(needle) => summary
                .qualified_type
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase()),
            None => true,
        })
        .collect();

    let mut objects = Vec::new();
    let mut unreadable = 0usize;
    for summary in &summaries {
        match store.get_object(&summary.uri) {
            Ok(object) => objects.push((summary, object)),
            Err(_) => unreadable += 1,
        }
    }
    let graph = graph::build(objects.iter().map(|(s, o)| (*s, o.as_reflect())));

    let rendered = match args.format.to_ascii_lowercase().as_str() {
        "json" => graph.to_json(),
        "dot" => graph.to_dot(),
        other => anyhow::bail!("unknown format `{other}` — `json` or `dot`"),
    };
    match &args.out {
        Some(path) => {
            std::fs::write(path, &rendered).with_context(|| format!("writing {}", path.display()))?;
            eprintln!(
                "{} node(s), {} edge(s) → {}",
                graph.nodes.len(),
                graph.edges.len(),
                path.display()
            );
        }
        None => print!("{rendered}"),
    }
    // Said on stderr so it never lands in the file: an object that could not be
    // read is a node the graph does not have, and a viewer showing a graph with
    // a hole in it should know there is one.
    let outside = graph.nodes.iter().filter(|n| !n.present).count();
    if outside > 0 {
        match &args.filter {
            Some(needle) => eprintln!(
                "{outside} referenced object(s) are outside the `{needle}` selection — drop \
                 `--filter` to bring them in"
            ),
            None => eprintln!("{outside} referenced object(s) are not in this package"),
        }
    }
    if unreadable > 0 {
        eprintln!(
            "{unreadable} object(s) could not be read, so their references are missing (try `--lenient`)"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// h5
// ---------------------------------------------------------------------------

/// Where each array's numbers really are, and — with `--fix` — a package that
/// says so.
///
/// The reading side is permissive on purpose: a renamed package still reads,
/// because the search opens the files beside it and asks which one holds the
/// dataset. That is the right behaviour and it has a cost — **every reader
/// pays for the search, every time**, and every reader that is not this one
/// simply fails. So this command exists to write the answer down: run it once
/// after renaming, and the package is ordinary again.
///
/// It is also the answer to "which file is this reading, actually", which is a
/// question a permissive reader has a duty to be able to answer plainly.
fn h5_command(args: H5Args, modes: Modes) -> Result<ExitCode> {
    let mut store = open(&args.file, &modes)?;

    // One row per *declared name*, not per array pointer: a package has
    // thousands of pointers and two or three files, and a person wants the
    // files.
    #[derive(Default)]
    struct Row {
        pointers: usize,
        found: Option<energyml_storage::Resolution>,
        proxies: std::collections::BTreeSet<String>,
        example: String,
    }
    let mut rows: std::collections::BTreeMap<String, Row> = std::collections::BTreeMap::new();

    for summary in store.resources() {
        let Ok(object) = store.get_object(&summary.uri) else {
            continue;
        };
        for location in store.arrays_of(object.as_reflect()) {
            let declared = location.file.clone();
            let row = rows.entry(declared).or_default();
            row.pointers += 1;
            if let Some(proxy) = &location.proxy_uuid {
                row.proxies.insert(proxy.to_ascii_lowercase());
            }
            if row.found.is_none() {
                row.example = location.path_in_file.clone();
                row.found = store.resolve_array(&location).ok();
            }
        }
    }

    if rows.is_empty() {
        println!("no external array in this package");
        return Ok(ExitCode::SUCCESS);
    }

    let mut missing: Vec<String> = Vec::new();
    for (declared, row) in &rows {
        let name = if declared.is_empty() {
            "(unnamed)"
        } else {
            declared
        };
        match &row.found {
            Some(found) if found.agrees() => {
                println!("{name}  {} pointer(s)  → {}", row.pointers, found.path.display());
            }
            Some(found) => {
                println!(
                    "{name}  {} pointer(s)  → {}  [{}]",
                    row.pointers,
                    found.path.display(),
                    found.origin.describe()
                );
            }
            None => {
                println!(
                    "{name}  {} pointer(s)  → NOT FOUND  (looking for `{}`)",
                    row.pointers, row.example
                );
                missing.push(declared.clone());
            }
        }
    }

    // Anything still missing, asked for by name — once, and reused for the
    // rest of the run.
    if args.interactive && !missing.is_empty() {
        use std::io::Write;
        for declared in &missing {
            let name = if declared.is_empty() {
                "(unnamed)"
            } else {
                declared
            };
            print!("path for `{name}` (blank to skip): ");
            std::io::stdout().flush().ok();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                break;
            }
            let path = line.trim();
            if path.is_empty() {
                continue;
            }
            store.external_files_mut().set(declared.clone(), path);
        }
        // Ask again, now that the caller has said where things are.
        for (declared, row) in rows.iter_mut() {
            if row.found.is_some() {
                continue;
            }
            for summary in store.resources() {
                let Ok(object) = store.get_object(&summary.uri) else {
                    continue;
                };
                for location in store.arrays_of(object.as_reflect()) {
                    if &location.file == declared && row.found.is_none() {
                        row.found = store.resolve_array(&location).ok();
                    }
                }
            }
        }
    }

    let Some(out) = args.fix else {
        let unresolved = rows.values().filter(|row| row.found.is_none()).count();
        return Ok(match unresolved {
            0 => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        });
    };

    // Writing the answer down. Two places carry a file name and both are
    // rewritten, because a package may use either — and a package that has
    // been through several tools may use both.
    let mut fixed = 0usize;
    for (declared, row) in &rows {
        let Some(found) = &row.found else { continue };
        if found.agrees() {
            continue;
        }
        let name = found.name();

        // The older form: the name lives in the `.rels` of an
        // `EpcExternalPartReference`, and `save` regenerates those from what
        // the store knows.
        for proxy in &row.proxies {
            store.register_external_file(proxy, name.clone());
            fixed += 1;
        }

        // The eml 2.3 form: the name is in the object, in every
        // `ExternalDataArrayPart.URI` that carries it.
        let users: Vec<String> = store
            .resources()
            .iter()
            .filter(|summary| {
                store
                    .get_object(&summary.uri)
                    .map(|object| {
                        store
                            .arrays_of(object.as_reflect())
                            .iter()
                            .any(|location| &location.file == declared)
                    })
                    .unwrap_or(false)
            })
            .map(|summary| summary.uri.clone())
            .collect();
        for uri in users {
            let object = store.edit(&uri)?;
            if rename_uris(object, declared, &name) > 0 {
                fixed += 1;
            }
        }
        println!("`{declared}` → `{name}`");
    }

    if fixed == 0 {
        println!("nothing to fix: every array names the file it is in");
        return Ok(ExitCode::SUCCESS);
    }

    // Carry over every file name the package *had*. A save regenerates the
    // `.rels` from what the store knows, and what it knows about external files
    // is `declared_files` — so a name that was only ever read out of a `.rels`
    // is dropped unless it is put back first. Repairing a package by losing
    // half its pointers would be a poor repair.
    let found: Vec<(String, String)> = store.external_file_map().into_iter().collect();
    for (uuid, file) in found {
        store.register_external_file(&uuid, file);
    }

    let before = unresolved_pointers(&store);
    let diagnostics = store.save_as(&out, &energyml_storage::SaveOptions::default())?;
    for diagnostic in &diagnostics {
        eprintln!("  [{:?}] {}", diagnostic.severity, diagnostic.message);
    }
    println!("{fixed} declaration(s) rewritten → {}", out.display());

    // Did the repair repair? Reopening and counting is the only answer that
    // does not take the writer's word for it — and the failure it guards
    // against is real: an `externalResource` relationship on a part that is not
    // a proxy has no counterpart in the writer, so a package that used one
    // comes back with fewer pointers than it went in with.
    let written = open(&out, &modes)?;
    let after = unresolved_pointers(&written);
    if after > before {
        eprintln!(
            "warning: {} array pointer(s) named a file before and name none now (was {before}). \
             The package used an `externalResource` relationship the writer does not reproduce; \
             `{}` still reads, because the search finds the file, but another tool may not.",
            after - before,
            out.display()
        );
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

/// How many array pointers name no file at all.
fn unresolved_pointers(store: &EpcStore) -> usize {
    store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .flat_map(|object| store.arrays_of(object.as_reflect()))
        .filter(|location| location.file.is_empty())
        .count()
}

/// Replace a file name wherever an object writes one, and say how many times.
///
/// Reflection rather than a typed walk, for the reason the whole workspace is
/// arranged around: the field is `URI` on an eml 2.3 `ExternalDataArrayPart`
/// and `PathInHdfFile` has no name at all in 2.0, and a version this build
/// learns tomorrow will be handled by the same code.
fn rename_uris(object: &mut dyn energyml_core::value::Reflect, from: &str, to: &str) -> usize {
    use energyml_core::value::{FieldValue, OwnedValue};

    let mut changed = 0usize;
    let fields: Vec<(&'static str, usize)> = object
        .type_info()
        .fields
        .iter()
        .map(|f| {
            let occurrences = object.field(f.name).map(|v| v.occurrences()).unwrap_or(0);
            (f.name, occurrences)
        })
        .collect();

    for (name, occurrences) in fields {
        // The scalar that carries a file name, whatever the release calls the
        // element around it.
        if name.eq_ignore_ascii_case("URI")
            && let Some(FieldValue::Scalar(scalar)) = object.field(name)
        {
            let text = scalar.to_string();
            let bare = text.rsplit(['/', '\\']).next().unwrap_or(&text);
            if text == from || bare == from {
                if object.set_field(name, OwnedValue::Str(to.to_string())).is_ok() {
                    changed += 1;
                }
                continue;
            }
        }
        // Descend. `field_object_mut` is the only mutable way in, so a list is
        // walked by index rather than by iterator.
        match occurrences {
            0 => {
                if let Some(child) = object.field_object_mut(name, None) {
                    changed += rename_uris(child, from, to);
                }
            }
            n => {
                for index in 0..n {
                    if let Some(child) = object.field_object_mut(name, Some(index)) {
                        changed += rename_uris(child, from, to);
                    }
                }
            }
        }
    }
    changed
}

/// The qualified type of a scene object, `resqml22.TriangulatedSetRepresentation`.
///
/// Read out of the ETP URI rather than from `SceneObject::qualified_type`,
/// which — despite its name and its doc comment — carries only the bare type
/// name. Left alone deliberately: that field is written into every export's
/// metadata (`type` in VTK field data, in GeoJSON properties, in an OBJ
/// comment), and changing what it holds would change files people already read.
fn qualified_type_of(object: &energyml_geometry::SceneObject) -> String {
    // The one reading of a uri into an identity — which also knows a dataspace
    // segment and a version qualifier.
    energyml_core::identity::ObjectId::parse(&object.uri)
        .map(|(_, id)| id.qualified_type())
        .unwrap_or_else(|_| object.qualified_type.clone())
}

/// The same string, with anything a file system objects to replaced.
///
/// A qualified type holds none of these today. It is here because the cost of
/// being wrong is a run that dies halfway through a batch on one object's name,
/// and the cost of being right is six characters in a match.
fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// relabel
// ---------------------------------------------------------------------------

/// Correct what a package's parts *declare*, without reading what they hold.
///
/// The case this exists for: a package written against a development drop —
/// `resqml 2.2_dev3` — whose parts declare the release's `schemaVersion="2.2"`,
/// because the drop's own schemas declare it too. Read as the release, such a
/// part **parses**: unknown children are skipped rather than rejected, so the
/// object comes back missing whatever the drop has and the release does not,
/// and saving that object writes the loss to disk.
///
/// Which is why this command never deserialises anything. It locates the root
/// element's tag, rewrites one attribute inside it, and leaves every other byte
/// alone — so there is no step at which data could be dropped. The `.rels` are
/// kept as they were, which is correct here and only here: the references did
/// not change, because nothing but a version string did.
///
/// ```text
/// energyml relabel model.epc --package resqml --schema-version 2.2_dev3
/// energyml relabel model.epc --package resqml --schema-version 2.2_dev3 -o fixed.epc
/// ```
///
/// Without `--out` it writes nothing and reports what it would do, the same
/// bargain `repair --apply` offers.
/// `energyml rename` — the quote, then the bill.
///
/// Nothing here decides anything about the package: `rename::plan` counts what
/// would change and says why it refuses, `rename::apply` carries it out, and
/// this prints. The one choice made here is which file `--from` means when the
/// argument is absent, which is about the command line and not about the EPC.
fn rename_command(args: RenameArgs, modes: Modes) -> Result<ExitCode> {
    use energyml_storage::rename;

    let mut store = open(&args.file, &modes)?;

    let from = match args.from {
        Some(name) => name,
        None => match store.external_files() {
            [only] => only.clone(),
            [] => anyhow::bail!(
                "{} declares no external data file — pass `--from <NAME>`",
                args.file.display()
            ),
            several => anyhow::bail!(
                "{} declares {} external data files ({}) — pass `--from <NAME>`",
                args.file.display(),
                several.len(),
                several.join(", ")
            ),
        },
    };

    let quote = rename::plan(&store, &from, &args.to);
    println!(
        "{} -> {}: {} field(s) in {} object(s), {} relationship(s)",
        quote.from,
        quote.to,
        quote.fields,
        quote.objects.len(),
        quote.relationships
    );
    for why in &quote.refusals {
        eprintln!("  refused: {why}");
    }
    if !quote.is_allowed() {
        return Ok(ExitCode::FAILURE);
    }
    if !args.apply {
        println!(
            "
nothing touched — pass `--apply` to carry it out"
        );
        return Ok(ExitCode::SUCCESS);
    }

    let done = rename::apply(&mut store, &quote)?;
    match &done.file {
        rename::FileMove::Moved(path) => println!("the data file is now {}", path.display()),
        // Not an error: a package whose `.h5` never shipped is already in that
        // state, and saying so is more use than failing.
        rename::FileMove::Absent => println!(
            "no file was found at `{}`; the package now names `{}` all the same",
            quote.from, quote.to
        ),
        rename::FileMove::NotOnDisk => println!("this package keeps its files elsewhere; nothing moved"),
    }

    let out = args.out.unwrap_or_else(|| args.file.clone());
    let diagnostics = store.save_as(&out, &energyml_storage::SaveOptions::default())?;
    for diagnostic in &diagnostics {
        eprintln!("  {diagnostic}");
    }
    println!("written to {}", out.display());
    Ok(ExitCode::SUCCESS)
}

fn relabel(args: RelabelArgs, modes: Modes) -> Result<ExitCode> {
    use energyml_storage::relabel;

    let mut store = open(&args.file, &modes)?;
    // **The decisions are the library's**, since 08/09/2026. They were here —
    // which parts belong to the package, which releases the file holds, the
    // refusal when it holds two — and that is logic in a binary, which this
    // workspace does not do. The front end needs the identical answers, and a
    // second copy of *when is a package too mixed to relabel* is the shape of
    // the bugs this repository keeps finding.
    let survey = relabel::survey(&store, &args.package, args.from.as_deref());

    if args.from.is_none()
        && let Some(why) = survey.refusal()
    {
        anyhow::bail!("{}: {why} — pass `--from <VERSION>`", args.file.display());
    }

    if survey.targets.is_empty() {
        println!(
            "no part of package `{}` in {}",
            survey.package,
            args.file.display()
        );
        if survey.undecided > 0 {
            println!(
                "{} part(s) declare a type this build cannot place in a package",
                survey.undecided
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    let outcome = relabel::apply(&mut store, &survey, &args.schema_version);
    for (qualified, was) in &outcome.replaced {
        println!("  {qualified}: {was} -> {}", args.schema_version);
    }
    for qualified in &outcome.added {
        println!(
            "  {qualified}: declared nothing -> {} (the attribute is required)",
            args.schema_version
        );
    }
    for why in &outcome.failed {
        eprintln!("  {why}");
    }

    println!(
        "\n{} part(s) of `{}`: {} rewritten, {} given the attribute they lacked, \
         {} already saying it, {} refused",
        survey.targets.len(),
        survey.package,
        outcome.replaced.len(),
        outcome.added.len(),
        outcome.already,
        outcome.failed.len()
    );
    if survey.undecided > 0 {
        println!(
            "{} part(s) declare a type this build cannot place in a package",
            survey.undecided
        );
    }

    let Some(out) = args.out else {
        println!("\nnothing written — pass `--out FILE` to save the corrected package");
        return Ok(ExitCode::SUCCESS);
    };
    // Two passes, and the library knows why — staging bytes protects the
    // content but leaves no object for the `.rels` to be built from, so the
    // package is reopened and saved again. See `relabel::save_reindexed`.
    let diagnostics = relabel::save_reindexed(
        &store,
        &out,
        Arc::new(registry::all()),
        &energyml_storage::SaveOptions::default(),
    )?;
    for diagnostic in &diagnostics {
        eprintln!("  {diagnostic}");
    }
    println!("written to {}", out.display());
    Ok(if outcome.failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Cut a package down to a chosen set of objects.
///
/// The command is argument parsing over `energyml_storage::extract`, as every
/// other one here is: the planning, the check and the writing are the library's,
/// and a binary that knew something the library did not would be a fault in the
/// layering.
fn extract_command(args: ExtractArgs, modes: Modes) -> Result<ExitCode> {
    let store = open(&args.file, &modes)?;
    let text = std::fs::read_to_string(&args.select)
        .with_context(|| format!("reading {}", args.select.display()))?;
    let wanted = read_manifest(&text).with_context(|| format!("reading {}", args.select.display()))?;

    let resources = store.resources();
    let mut objects = Vec::new();
    for summary in &resources {
        if let Ok(object) = store.get_object(&summary.uri) {
            objects.push((summary, object));
        }
    }
    let graph = energyml_storage::graph::build(objects.iter().map(|(s, o)| (*s, o.as_reflect())));
    let plan = energyml_storage::extract::plan(&resources, &graph, &wanted);

    // The report before the file, always — and before the *decision* to write,
    // when it is a dry run. A curated package that lost something in silence is
    // worse than one nobody curated: no one would ever learn it was missing.
    println!("{} object(s) kept, {} left", plan.kept.len(), plan.left);
    if plan.left > 0 {
        println!(
            "  of those left, {} are referred to by nothing kept — the cut is clean there",
            plan.clean
        );
    }
    for uri in &plan.unknown {
        println!("  asked for and not in this package: {uri}");
    }
    let cut: Vec<_> = plan.broken.iter().filter(|one| one.in_source).collect();
    let carried: Vec<_> = plan.broken.iter().filter(|one| !one.in_source).collect();
    if !cut.is_empty() {
        println!("  {} reference(s) this cut would break:", cut.len());
        for one in cut.iter().take(20) {
            println!("    {} -> {} ({})", one.from, one.to, one.path);
        }
        if cut.len() > 20 {
            println!("    … and {} more", cut.len() - 20);
        }
    }
    if !carried.is_empty() {
        // Said, and told apart: the source already named these without holding
        // them, so blaming the cut for them would train people to ignore the
        // list that matters.
        println!(
            "  {} reference(s) already left the package before this cut",
            carried.len()
        );
    }

    if args.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    // A cut that breaks what was whole is refused rather than written. The tool
    // does not produce a package that does not open and hope somebody checks.
    if !plan.is_clean() {
        anyhow::bail!(
            "this selection is not closed: {} reference(s) would be broken. \
             Add what they name, or pass --dry-run to inspect.",
            cut.len()
        );
    }

    let arrays = if args.keep_arrays {
        energyml_storage::extract::Arrays::Leave
    } else {
        energyml_storage::extract::Arrays::Copy
    };
    let done = energyml_storage::extract::extract(
        &store,
        &wanted,
        &args.out,
        arrays,
        &energyml_storage::epc::SaveOptions::default(),
    )
    .with_context(|| format!("writing {}", args.out.display()))?;
    for uri in &done.unreadable {
        eprintln!("could not be read, so was not written: {uri}");
    }
    // Named, never a silent shortfall: a package without the numbers it
    // promises looks exactly like one with them, and the only moment anybody
    // would find out is when a viewer draws nothing.
    if !done.arrays_missed.is_empty() {
        eprintln!(
            "{} array(s) could not be copied — the package points at them and does not carry them:",
            done.arrays_missed.len()
        );
        for one in done.arrays_missed.iter().take(10) {
            eprintln!("  {one}");
        }
        if done.arrays_missed.len() > 10 {
            eprintln!("  … and {} more", done.arrays_missed.len() - 10);
        }
        eprintln!(
            "  (a build without the `hdf5` feature cannot copy them; `--keep-arrays` points at the source file instead)"
        );
    }
    println!(
        "{} → {} object(s), {} array(s)",
        args.out.display(),
        done.plan.kept.len(),
        done.arrays
    );
    Ok(ExitCode::SUCCESS)
}

/// The uris a manifest names.
///
/// Only `chosen` is read: what the closure worked out is written in the file so
/// it can be *read*, and recomputed here because the package may have moved
/// since. A web viewer writes it.
fn read_manifest(text: &str) -> Result<Vec<String>> {
    let root: serde_json::Value = serde_json::from_str(text)?;
    let version = root.get("manifest").and_then(|v| v.as_u64());
    if version != Some(1) {
        anyhow::bail!("this is a manifest of version {version:?}; this build reads 1");
    }
    let list = root
        .get("chosen")
        .and_then(|v| v.as_array())
        .context("`chosen` is missing, or is not a list")?;
    let mut out = Vec::new();
    for (index, one) in list.iter().enumerate() {
        match one.as_str() {
            Some(uri) if !uri.is_empty() => out.push(uri.to_string()),
            // Named, not skipped: a decision that comes back shorter than it was
            // written is a decision nobody can trust twice.
            _ => eprintln!("chosen[{index}] is not a uri, and was left out"),
        }
    }
    Ok(out)
}
