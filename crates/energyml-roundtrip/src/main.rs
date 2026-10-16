//! Reads real EnergyML documents, writes them back, and reports anything lost.
//!
//! ```text
//! cargo run -p energyml-roundtrip -- testdata/epc                 # every EPC in a folder
//! cargo run -p energyml-roundtrip -- some.epc part.xml --report r.md
//! ```
//!
//! Files written by fesapi, resqpy or any other implementation are the only
//! honest test of a reader: the schemas say what is legal, they say what is
//! actually produced. Each XML part is deserialized through the registry,
//! serialized back, and the two are compared *structurally* - see [`shape`].
//!
//! Exit code is non-zero when a part failed to read or lost information, so
//! this can sit in CI once a corpus is available to it.

mod shape;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use energyml_core::registry::{self, ReadMode, Registry};
use energyml_core::xml::XmlWriteOptions;

use shape::{Diff, DiffKind};

#[derive(Parser)]
#[command(name = "energyml-roundtrip", about, version)]
struct Cli {
    /// EPC files, XML files, or folders holding them.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Write a Markdown report here.
    #[arg(long)]
    report: Option<PathBuf>,

    /// Print every finding as it is found, not just the summary.
    #[arg(long, short)]
    verbose: bool,

    /// Stop listing findings for one part after this many.
    #[arg(long, default_value_t = 20)]
    max_findings: usize,

    /// Write every re-serialized part here, to look at one by hand.
    #[arg(long)]
    dump: Option<PathBuf>,
}

/// A loss the schema makes unavoidable, because the document puts something
/// where the schema declares nothing to hold it.
///
/// The list is short on purpose, and every entry cites the schema that settles
/// it. It is **not** somewhere to put an inconvenient finding: a loss belongs
/// here only when the *document* is at fault, and the reason has to say where
/// the XSD says so. Anything else is the reader's bug and must stay red.
struct Accepted {
    /// The path the diff is reported on, spelled exactly as the report prints it.
    path: &'static str,
    /// A substring the part's origin must contain, so the exemption cannot
    /// spread to another object that happens to share the path.
    part: &'static str,
    /// Why no conformant reader could have kept it.
    reason: &'static str,
}

const ACCEPTED_LOSSES: &[Accepted] = &[Accepted {
    path: "{http://www.energistics.org/energyml/data/prodmlv2}Key@keyword",
    part: "TimeSeriesData",
    reason: "PRODML 2.2 declares `TimeSeriesData/Key` with `type=\"xs:string\"` \
             (`TimeSeriesData.xsd`): that element carries no attribute. fesapi \
             writes `keyword` and `xsi:type=\"prodml23:KeywordValueStruct\"` on it, \
             which belong to the `Key` of `TimeSeriesStatistic` - the other `Key` in \
             the same package, whose type is `KeywordValueStruct`. No conformant \
             reader can read that attribute back: the document is at fault, not the \
             reading.",
}];

/// The reason a loss is accepted, if it is one.
fn accepted_reason(diff: &Diff, origin: &str) -> Option<&'static str> {
    if !diff.kind.is_loss() {
        return None;
    }
    ACCEPTED_LOSSES
        .iter()
        .find(|a| a.path == diff.path && origin.contains(a.part))
        .map(|a| a.reason)
}

/// What happened to one XML part.
struct PartResult {
    /// Where it came from: `archive.epc!part.xml` or a plain path.
    origin: String,
    /// Qualified type it was read as, when it could be identified.
    qualified_type: Option<String>,
    /// Differences between the original and the round trip.
    diffs: Vec<Diff>,
    /// Losses the schema makes unavoidable, with the reason. Kept out of
    /// [`PartResult::diffs`] so they cannot fail the run, and reported on
    /// their own so they cannot be forgotten.
    accepted: Vec<(Diff, &'static str)>,
    /// Why the part could not be handled at all.
    failure: Option<String>,
    /// Something worth knowing that did not stop the check.
    note: Option<String>,
    /// Elements in the original, as a rough measure of what was exercised.
    elements: usize,
}

impl PartResult {
    fn losses(&self) -> usize {
        self.diffs.iter().filter(|d| d.kind.is_loss()).count()
    }
    fn additions(&self) -> usize {
        self.diffs
            .iter()
            .filter(|d| {
                !d.kind.is_loss() && d.kind != DiffKind::XsiTypeDropped && d.kind != DiffKind::RootRenamed
            })
            .count()
    }
    /// Redundant `xsi:type` attributes the writer did not repeat. Counted
    /// apart: they are normalization, not a difference in the data.
    fn redundant(&self) -> usize {
        self.diffs
            .iter()
            .filter(|d| d.kind == DiffKind::XsiTypeDropped)
            .count()
    }
    fn is_clean(&self) -> bool {
        self.failure.is_none() && self.losses() == 0 && self.additions() == 0 && self.accepted.is_empty()
    }
}

/// Every schema version the workspace generates.
///
/// A second copy of this list used to live here, beside the one in the CLI.
/// Neither was checked against anything, so a new release left out of one of
/// them made this tool report the part as an unreadable loss - a *false* loss
/// report, which is the worst thing a round-trip tool can produce.
fn registry() -> Registry {
    energyml_schemas::all()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let reg = registry();

    let mut results: Vec<PartResult> = Vec::new();
    for input in &cli.inputs {
        collect(input, &reg, &mut results, &cli)?;
    }

    if results.is_empty() {
        anyhow::bail!("no XML part found in the given inputs");
    }

    let report = render_report(&results, cli.max_findings);
    if let Some(path) = &cli.report {
        fs::write(path, &report).with_context(|| format!("writing {}", path.display()))?;
        println!("report written to {}", path.display());
    }
    println!("{}", summary(&results));

    let failed = results.iter().filter(|r| r.failure.is_some()).count();
    let lossy = results.iter().filter(|r| r.losses() > 0).count();
    if failed > 0 || lossy > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Walk one input, whatever it is.
fn collect(path: &Path, reg: &Registry, out: &mut Vec<PartResult>, cli: &Cli) -> Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for entry in entries {
            collect(&entry, reg, out, cli)?;
        }
        return Ok(());
    }

    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("epc") | Some("zip") => collect_epc(path, reg, out, cli),
        Some("xml") => {
            let bytes = fs::read(path)?;
            out.push(check_part(&path.display().to_string(), &bytes, None, reg, cli));
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Every XML part of an EPC, typed by `[Content_Types].xml` the way a real
/// reader does it.
fn collect_epc(path: &Path, reg: &Registry, out: &mut Vec<PartResult>, cli: &Cli) -> Result<()> {
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).with_context(|| format!("reading {}", path.display()))?;

    let mut content_types = String::new();
    if let Ok(mut entry) = zip.by_name("[Content_Types].xml") {
        entry.read_to_string(&mut content_types).ok();
    }
    let overrides = parse_content_types(&content_types);

    let names: Vec<String> = zip
        .file_names()
        .map(|name| name.map(|n| n.into_owned()))
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("reading the part names of {}", path.display()))?;
    let mut parts: Vec<String> = names
        .into_iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".xml"))
        .collect();
    parts.sort();

    for name in parts {
        if name.starts_with('[') || name.contains("_rels/") || name.starts_with("docProps") {
            continue;
        }
        let mut bytes = Vec::new();
        zip.by_name(&name)?.read_to_end(&mut bytes)?;
        let declared = overrides.get(&format!("/{name}")).cloned();
        let origin = format!("{}!{name}", path.display());
        out.push(check_part(&origin, &bytes, declared, reg, cli));
    }
    Ok(())
}

/// `<Override PartName="/x.xml" ContentType="…"/>` → a map.
fn parse_content_types(xml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Empty(e)) | Ok(quick_xml::events::Event::Start(e))
                if e.name().as_ref().rsplit(|b| *b == b':').next() == Some(b"Override") =>
            {
                {
                    let mut part = None;
                    let mut ty = None;
                    for attribute in e.attributes().flatten() {
                        let value = String::from_utf8_lossy(&attribute.value).into_owned();
                        match attribute.key.as_ref() {
                            b"PartName" => part = Some(value),
                            b"ContentType" => ty = Some(value),
                            _ => {}
                        }
                    }
                    if let (Some(p), Some(t)) = (part, ty) {
                        // Some writers omit the leading slash the OPC spec
                        // requires. Normalising here beats failing to type a
                        // part over a punctuation detail.
                        let key = if p.starts_with('/') { p } else { format!("/{p}") };
                        out.insert(key, t);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Read one part, write it back, compare.
fn check_part(
    origin: &str,
    bytes: &[u8],
    declared_content_type: Option<String>,
    reg: &Registry,
    cli: &Cli,
) -> PartResult {
    let verbose = cli.verbose;
    let mut result = PartResult {
        origin: origin.to_string(),
        qualified_type: None,
        diffs: Vec::new(),
        accepted: Vec::new(),
        failure: None,
        note: None,
        elements: 0,
    };

    let before = match shape::of(bytes) {
        Ok(s) => s,
        Err(e) => {
            result.failure = Some(format!("the original document is unreadable: {e}"));
            return result;
        }
    };
    result.elements = before.elements.values().sum();

    // How a real EPC reader picks the type; falling back to the root element
    // when the package has no `[Content_Types].xml` entry for this part.
    // How a real EPC reader picks the type. `[Content_Types].xml` is the
    // authority - but it is written by hand-rolled code in every
    // implementation, so when it names something unusable the root element is
    // the better authority. The fallback is recorded, not hidden.
    let object = match declared_content_type.as_deref() {
        // Strictly. A round-trip measures whether reading and writing preserve
        // the document, and a tolerant read invents what the document omitted -
        // which the writer would then emit, turning an absent element into an
        // empty one and reporting the file as *changed by us*. A part this
        // refuses is a part that cannot be round-tripped without inventing, and
        // saying so is the tool's job.
        Some(ct) => match reg.read_xml_as(ct, bytes, ReadMode::Strict).map(|(o, _)| o) {
            Ok(o) => Ok(o),
            Err(first) => match read_by_root(&before.root, before.schema_version.as_deref(), reg, bytes) {
                Ok(o) => {
                    result.note = Some(format!(
                        "contentType `{ct}` unusable ({first}) - type deduced from the root element"
                    ));
                    Ok(o)
                }
                Err(second) => Err(format!("contentType `{ct}`: {first}; racine: {second}")),
            },
        },
        None => read_by_root(&before.root, before.schema_version.as_deref(), reg, bytes),
    };
    let object = match object {
        Ok(o) => o,
        Err(e) => {
            result.failure = Some(format!("cannot read: {e}"));
            if verbose {
                eprintln!("[failed] {origin}: {}", result.failure.as_deref().unwrap_or(""));
            }
            return result;
        }
    };
    result.qualified_type = object.as_reflect().type_info().qualified_type.map(str::to_string);

    let written = match registry::to_part(object.as_ref(), XmlWriteOptions::default()) {
        Ok(w) => w,
        Err(e) => {
            result.failure = Some(format!("cannot write: {e}"));
            return result;
        }
    };
    if let Some(dir) = &cli.dump {
        let name = origin.replace(['/', '\\', '!', ':'], "_");
        let _ = fs::create_dir_all(dir);
        let _ = fs::write(dir.join(format!("{name}.original.xml")), bytes);
        let _ = fs::write(dir.join(format!("{name}.written.xml")), written.as_bytes());
    }

    let after = match shape::of(written.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            result.failure = Some(format!("the written output is unreadable: {e}"));
            return result;
        }
    };

    let mut diffs = shape::compare(&before, &after);
    let mut accepted = Vec::new();
    diffs.retain(|diff| match accepted_reason(diff, origin) {
        Some(reason) => {
            accepted.push((diff.clone(), reason));
            false
        }
        None => true,
    });
    result.diffs = diffs;
    result.accepted = accepted;
    if verbose && !result.diffs.is_empty() {
        eprintln!("[diff]  {origin}: {} difference(s)", result.diffs.len());
    }
    Ok::<(), ()>(()).ok();
    result
}

/// A dotted version as numbers, so `2.0.1` sorts after `2.0` and before `2.2`.
/// A segment that is not a number counts as zero rather than making the whole
/// version incomparable.
fn version_parts(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

/// How good a candidate this release is for a document declaring `declared`,
/// larger being better.
///
/// The document's `schemaVersion` is the only version signal it carries, and
/// it is a *hint*, not a decision: it is written by hand-rolled code in every
/// implementation, and a producer may stamp a release we do not generate -
/// fesapi writes `2.3` on PRODML 2.2 documents. So the rule is the newest
/// release that does not claim to be more recent than the document, and,
/// when none qualifies, simply the newest.
///
/// Order matters more than it looks. Every version of a package shares one
/// namespace URI, so with the versions sorted the other way a RESQML 2.2
/// document read by root element was read as 2.0.1 - which succeeds, because
/// an unknown child is skipped for forward compatibility - and every element
/// 2.2 added came back missing. The round trip then reported *its own* choice
/// of reader as the file losing information.
fn version_rank(
    info: &energyml_core::reflect::TypeInfo,
    declared: Option<&[u32]>,
) -> (bool, Vec<u32>, Vec<u32>) {
    // `schema_version` is the form identifiers use - `2.0` for RESQML 2.0.1 -
    // and it is what a document's `schemaVersion` is comparable to.
    let schema = version_parts(info.schema_version);
    let not_newer = declared.is_some_and(|want| schema.as_slice() <= want);
    (not_newer, schema, version_parts(info.version))
}

/// Identify a standalone document from its root element.
///
/// The namespace URI gives the package but not the version - every RESQML
/// version shares one URI - so the candidates are tried in turn and the first
/// that reads wins. `declared_version` is the root's `schemaVersion`, used to
/// order them; see [`version_rank`].
fn read_by_root(
    root: &str,
    declared_version: Option<&str>,
    reg: &Registry,
    bytes: &[u8],
) -> std::result::Result<Box<dyn registry::DynObject>, String> {
    let (uri, local) = match root.strip_prefix('{').and_then(|r| r.split_once('}')) {
        Some((uri, local)) => (uri, local),
        None => return Err(format!("root `{root}` has no namespace")),
    };
    let package = match uri.rsplit('/').next() {
        Some("resqmlv2") => "resqml",
        Some("commonv2") => "eml",
        Some("witsmlv2") => "witsml",
        Some("prodmlv2") => "prodml",
        _ => return Err(format!("unknown namespace `{uri}`")),
    };

    // `xml_name` is the element the schema declares; `object_type` is the
    // complexType. RESQML 2.0.1 spells them differently (`DiscreteProperty`
    // versus `obj_DiscreteProperty`) and files in the wild use both as the
    // root tag, so both are accepted.
    let mut candidates: Vec<&'static energyml_core::reflect::TypeInfo> = reg
        .types()
        .filter(|i| i.is_top_level && i.package == package && (i.xml_name == local || i.object_type == local))
        .collect();
    if candidates.is_empty() {
        return Err(format!("no top-level type `{local}` in package `{package}`"));
    }
    let declared = declared_version.map(version_parts);
    candidates.sort_by_key(|i| std::cmp::Reverse(version_rank(i, declared.as_deref())));

    let mut last = String::new();
    for info in candidates {
        let Some(content_type) = info.content_type else {
            continue;
        };
        match reg
            .read_xml_as(content_type, bytes, ReadMode::Strict)
            .map(|(o, _)| o)
        {
            Ok(object) => return Ok(object),
            Err(e) => last = format!("{} {}: {e}", info.package, info.version),
        }
    }
    Err(if last.is_empty() {
        format!("`{local}` illisible")
    } else {
        last
    })
}

fn summary(results: &[PartResult]) -> String {
    let total = results.len();
    let failed = results.iter().filter(|r| r.failure.is_some()).count();
    let lossy = results.iter().filter(|r| r.losses() > 0).count();
    let noisy = results
        .iter()
        .filter(|r| r.failure.is_none() && r.losses() == 0 && r.additions() > 0)
        .count();
    // A part whose only difference is an accepted loss is not equivalent - it
    // came back without something the document carried - so it gets its own
    // bucket rather than being rounded up into the good news.
    let equivalent = results.iter().filter(|r| r.is_clean()).count();
    let tolerated = results
        .iter()
        .filter(|r| r.failure.is_none() && r.losses() == 0 && r.additions() == 0 && !r.accepted.is_empty())
        .count();
    let redundant: usize = results.iter().map(|r| r.redundant()).sum();
    let accepted: usize = results.iter().map(|r| r.accepted.len()).sum();
    let mut out = format!(
        "{total} part(s): {equivalent} equivalent, {noisy} with additions, \
         {lossy} with losses, {failed} unreadable - {redundant} redundant xsi:type not rewritten"
    );
    if accepted > 0 {
        let _ = write!(out, ", {accepted} accepted loss(es) on {tolerated} part(s)");
    }
    out
}

fn render_report(results: &[PartResult], max_findings: usize) -> String {
    let mut out = String::new();
    out.push_str("# Round trip on real files\n\n");
    out.push_str(
        "Each XML part is deserialised then re-serialised, and the two documents are \
         compared on their *structure*: which elements, in which namespace, how many times, \
         with which attributes and values. Prefixes, attribute order, `2.0` written as \
         `2` and insignificant whitespace do not count - they carry no information.\n\n",
    );
    out.push_str(&format!("**{}**\n\n", summary(results)));

    // Aggregate: the same defect usually shows up on hundreds of parts, so the
    // report leads with the pattern and only then lists the parts.
    let mut by_pattern: BTreeMap<(DiffKind, String), (usize, Vec<String>)> = BTreeMap::new();
    for result in results {
        for diff in &result.diffs {
            let entry = by_pattern.entry((diff.kind, diff.path.clone())).or_default();
            entry.0 += 1;
            if entry.1.len() < 3 {
                entry.1.push(result.origin.clone());
            }
        }
    }

    if !by_pattern.is_empty() {
        out.push_str("## Differences, by pattern\n\n");
        out.push_str("| Type | Path | Occurrences | Examples |\n|---|---|---:|---|\n");
        let mut rows: Vec<_> = by_pattern.iter().collect();
        rows.sort_by_key(|((kind, path), (count, _))| (*kind, usize::MAX - count, path.clone()));
        for ((kind, path), (count, examples)) in rows.iter().take(60) {
            out.push_str(&format!(
                "| {} | `{}` | {} | {} |\n",
                kind.label(),
                path,
                count,
                examples
                    .iter()
                    .map(|e| format!("`{}`", short(e)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if rows.len() > 60 {
            out.push_str(&format!("\n… and {} more patterns.\n", rows.len() - 60));
        }
        out.push('\n');
    }

    // Accepted losses get their own section rather than a footnote: they do not
    // fail the run, so the report is the only place they exist.
    let mut by_reason: BTreeMap<&'static str, (usize, Vec<String>)> = BTreeMap::new();
    for result in results {
        for (diff, reason) in &result.accepted {
            let entry = by_reason.entry(reason).or_default();
            entry.0 += 1;
            if entry.1.len() < 3 {
                entry
                    .1
                    .push(format!("`{}` - `{}`", short(&result.origin), diff.path));
            }
        }
    }
    if !by_reason.is_empty() {
        out.push_str("## Accepted losses\n\n");
        out.push_str(
            "What the document carries and no conformant reader can read back, because the \
             schema declares nothing at that place to receive it. These differences do not \
             fail the check - but they remain differences, and each one cites the schema \
             that settles it.\n\n",
        );
        for (reason, (count, examples)) in &by_reason {
            out.push_str(&format!("- {reason} - {count} occurrence(s)\n"));
            for example in examples {
                out.push_str(&format!("  - {example}\n"));
            }
        }
        out.push('\n');
    }

    let noted: Vec<&PartResult> = results.iter().filter(|r| r.note.is_some()).collect();
    if !noted.is_empty() {
        out.push_str("## Notes\n\n");
        let mut by_note: BTreeMap<String, usize> = BTreeMap::new();
        for result in &noted {
            *by_note
                .entry(result.note.clone().unwrap_or_default())
                .or_default() += 1;
        }
        for (note, count) in &by_note {
            out.push_str(&format!("- {note} - {count} part(s)\n"));
        }
        out.push('\n');
    }

    let failures: Vec<&PartResult> = results.iter().filter(|r| r.failure.is_some()).collect();
    if !failures.is_empty() {
        out.push_str("## Unreadable parts\n\n");
        let mut by_reason: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for failure in &failures {
            by_reason
                .entry(failure.failure.clone().unwrap_or_default())
                .or_default()
                .push(failure.origin.clone());
        }
        for (reason, origins) in &by_reason {
            out.push_str(&format!("- **{}** - {} part(s)\n", reason, origins.len()));
            for origin in origins.iter().take(5) {
                out.push_str(&format!("  - `{}`\n", short(origin)));
            }
        }
        out.push('\n');
    }

    out.push_str("## Detail by part\n\n");
    for result in results {
        if result.is_clean() {
            continue;
        }
        out.push_str(&format!(
            "### `{}`\n\n- type: {}\n- elements: {}\n",
            short(&result.origin),
            result.qualified_type.as_deref().unwrap_or("not identified"),
            result.elements
        ));
        if let Some(failure) = &result.failure {
            out.push_str(&format!("- **failure**: {failure}\n\n"));
            continue;
        }
        let _ = writeln!(
            out,
            "- differences: {} loss(es), {} addition(s), {} redundant xsi:type\n",
            result.losses(),
            result.additions(),
            result.redundant()
        );
        for diff in result.diffs.iter().take(max_findings) {
            out.push_str(&format!(
                "  - {} `{}`: expected `{}`, got `{}`\n",
                diff.kind.label(),
                diff.path,
                diff.expected,
                diff.actual
            ));
        }
        if result.diffs.len() > max_findings {
            out.push_str(&format!("  - … and {} more\n", result.diffs.len() - max_findings));
        }
        for (diff, _) in result.accepted.iter().take(max_findings) {
            out.push_str(&format!(
                "  - {} `{}`: expected `{}`, got `{}` - **accepted**\n",
                diff.kind.label(),
                diff.path,
                diff.expected,
                diff.actual
            ));
        }
        out.push('\n');
    }

    let equivalent = results.iter().filter(|r| r.is_clean()).count();
    out.push_str(&format!(
        "## Equivalent parts\n\n{equivalent} part(s) come back with the same information - \
         give or take the redundant `xsi:type`, which this report counts separately.\n"
    ));
    out
}

/// Keep report tables readable: only the archive and the part matter.
fn short(origin: &str) -> String {
    match origin.rsplit_once('/') {
        Some((head, tail)) => match head.rsplit_once('/') {
            Some((_, parent)) => format!("{parent}/{tail}"),
            None => format!("{head}/{tail}"),
        },
        None => origin.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry entry a RESQML 2.2 document should be read as, and the one
    /// it used to be read as instead.
    fn resqml(local: &str) -> Vec<&'static energyml_core::reflect::TypeInfo> {
        let reg = registry();
        reg.types()
            .filter(|i| {
                i.is_top_level && i.package == "resqml" && (i.xml_name == local || i.object_type == local)
            })
            .collect()
    }

    /// The defect this ordering exists for: RESQML 2.0.1 and 2.2 share one
    /// namespace URI and both declare a `BoundaryFeature` element, 2.0.1 reads
    /// a 2.2 document without complaining - an unknown child is skipped for
    /// forward compatibility - and `IsWellKnown`, which only 2.2 has, came back
    /// missing. Eight parts of the corpus were reported as lossy for it.
    #[test]
    fn the_declared_schema_version_picks_the_release() {
        let mut candidates = resqml("BoundaryFeature");
        assert!(candidates.len() > 1, "the test needs both releases");
        let declared = version_parts("2.2");
        candidates.sort_by_key(|i| std::cmp::Reverse(version_rank(i, Some(&declared))));
        assert_eq!(candidates[0].version, "2.2");
    }

    /// And the older one still wins when the document says so.
    #[test]
    fn an_older_document_is_read_as_its_own_release() {
        let mut candidates = resqml("BoundaryFeature");
        let declared = version_parts("2.0");
        candidates.sort_by_key(|i| std::cmp::Reverse(version_rank(i, Some(&declared))));
        assert_eq!(candidates[0].version, "2.0.1");
    }

    /// Every PRODML release that declares a `FluidSystem`, for the two tests
    /// below.
    fn prodml_fluid_systems() -> Vec<&'static energyml_core::reflect::TypeInfo> {
        let reg = registry();
        reg.types()
            .filter(|i| i.is_top_level && i.package == "prodml" && i.xml_name == "FluidSystem")
            .collect()
    }

    /// A release we do not generate falls back to the newest that does not
    /// claim to be more recent.
    ///
    /// **The example had to change on 21/09, and that is the point of the
    /// note.** This test used to declare `2.3` and expect `2.2`, because 2.3
    /// was a release nobody here generated - fesapi stamps it on PRODML 2.2
    /// documents, and reading those as 2.0 dropped everything 2.2 added.
    /// Generating PRODML 2.3 made that expectation false *and right to be
    /// false*: a document that says 2.3 must now be read as 2.3. So the
    /// property is retested where it still bites - `2.1`, which no crate here
    /// generates and which must land on 2.0 rather than on the newest of all.
    #[test]
    fn an_unknown_release_falls_back_to_the_newest_below_it() {
        let mut candidates = prodml_fluid_systems();
        assert!(
            candidates.len() > 2,
            "the test needs three releases to tell a fallback from a maximum"
        );
        let declared = version_parts("2.1");
        candidates.sort_by_key(|i| std::cmp::Reverse(version_rank(i, Some(&declared))));
        assert_eq!(candidates[0].version, "2.0");
    }

    /// And a release we *do* generate is read as itself, not as a fallback.
    ///
    /// The other half of the rule above, and the one that changed the day
    /// PRODML 2.3 arrived. Worth its own test because the two are easy to
    /// confuse: *the newest that does not claim to be more recent* happens to
    /// be the exact match whenever the exact match exists, and a ranking that
    /// only ever fell back would pass every test of the first kind.
    #[test]
    fn a_release_we_generate_is_read_as_itself() {
        let mut candidates = prodml_fluid_systems();
        let declared = version_parts("2.3");
        candidates.sort_by_key(|i| std::cmp::Reverse(version_rank(i, Some(&declared))));
        assert_eq!(candidates[0].version, "2.3");
    }
}
