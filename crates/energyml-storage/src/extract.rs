//! Taking a chosen set of objects out of a package, and saying what that cost.
//!
//! # Why a package can be cut at all
//!
//! Everything a store holds ends up in a permanent one — an ETP server, an OSDU
//! catalogue — and not everything deserves to. What went through a sandbox is
//! attempts, resolutions and realisations; what is versed is a decision. The
//! partial EPC is how that decision is exercised without a server:
//! it carries exactly the same list, and reads back with the same tools.
//!
//! # The cut is planned before it is made
//!
//! [`plan`] answers, from the reference graph alone, what the new package would
//! hold and what it would break. It touches no file, so a caller can show the
//! consequences before anybody commits to them — which is the whole point of a
//! curation tool, and the reason this is not simply "write these parts".
//!
//! # Reporting is half the job
//!
//! A curated package that loses something in silence is **worse** than one that
//! was never curated: nobody will ever learn the thing was missing. So the plan
//! carries three numbers a person can act on — what is kept, what is left, and
//! **how much of what is left nothing kept refers to**, which is the one that
//! says the cut is clean — and it names every reference the cut would break.
//!
//! # Two references that leave the package are not the same news
//!
//! A kept object naming something the new package will not hold is either:
//!
//! - **the cut's doing** — the source held it and the choice left it out. That
//!   is a broken reference somebody has to decide about;
//! - **already the case in the source** — the package names an object it never
//!   held. A well-known property kind is the ordinary example, and a
//!   cross-package model boundary the other. Carrying that over changes nothing
//!   and is not the extraction's fault.
//!
//! [`Broken::in_source`] tells them apart, because a report that mixed them
//! would blame the cut for the package's own state.

use crate::epc::{EpcStore, SaveOptions};
use crate::error::{Error, Result};
use crate::graph::Graph;
use crate::resource::ObjectSummary;

/// A reference a kept object makes that the new package will not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// The kept object holding the reference.
    pub from: String,
    /// What it names.
    pub to: String,
    /// Where inside `from` — `Grid2dPatch.SupportingRepresentation`.
    pub path: String,
    /// Whether the **source** package held the target.
    ///
    /// `true` is the cut's doing and wants a decision. `false` was already so
    /// before anybody chose anything.
    pub in_source: bool,
}

/// What cutting a package to a chosen set would come to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// What the new package would hold, in the order the source lists it.
    pub kept: Vec<String>,
    /// Asked for, and the source does not hold it.
    ///
    /// Named rather than dropped: a decision that names an object the package
    /// lost has to be a visible fact, not a smaller list.
    pub unknown: Vec<String>,
    /// How many objects of the source stay behind.
    pub left: usize,
    /// Of those, how many **nothing kept refers to**.
    ///
    /// The number that says the cut is clean. `left == clean` means the chosen
    /// set is closed and the new package is self-contained.
    pub clean: usize,
    /// Every reference the new package would leave unresolved.
    pub broken: Vec<Broken>,
}

impl Plan {
    /// Whether the cut breaks nothing that was whole.
    ///
    /// References already unresolved in the source do not count: a cut is not
    /// answerable for a package that was already naming what it did not hold.
    pub fn is_clean(&self) -> bool {
        !self.broken.iter().any(|one| one.in_source)
    }
}

/// Work out what a cut would do, without touching a file.
///
/// `wanted` is the closed set the caller decided on — this does not close it
/// again. Two implementations of one walk would be two answers to the same
/// question; what this does instead is **check** the result, which is the
/// cheaper and more useful half: a set that is not closed is reported reference
/// by reference rather than silently repaired.
pub fn plan(resources: &[ObjectSummary], graph: &Graph, wanted: &[String]) -> Plan {
    let held: std::collections::HashSet<&str> =
        resources.iter().map(|one| one.uri.as_str()).collect();
    let mut asked: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut unknown = Vec::new();
    for uri in wanted {
        if held.contains(uri.as_str()) {
            asked.insert(uri.clone());
        } else if !unknown.contains(uri) {
            unknown.push(uri.clone());
        }
    }

    // In the source's order, not the caller's: a package lists its parts in an
    // order somebody chose, and a cut has no reason to shuffle them.
    let kept: Vec<String> = resources
        .iter()
        .filter(|one| asked.contains(&one.uri))
        .map(|one| one.uri.clone())
        .collect();

    let present: std::collections::HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|node| node.present)
        .map(|node| node.uri.as_str())
        .collect();

    let mut broken = Vec::new();
    for edge in &graph.edges {
        if !asked.contains(&edge.from) || asked.contains(&edge.to) {
            continue;
        }
        broken.push(Broken {
            from: edge.from.clone(),
            to: edge.to.clone(),
            path: edge.path.clone(),
            in_source: present.contains(edge.to.as_str()),
        });
    }

    // What nothing kept points at. Counted from the edges rather than from a
    // second walk: an object is "clean to leave" exactly when no kept object
    // names it.
    let named_by_kept: std::collections::HashSet<&str> = graph
        .edges
        .iter()
        .filter(|edge| asked.contains(&edge.from))
        .map(|edge| edge.to.as_str())
        .collect();
    let left: Vec<&ObjectSummary> = resources
        .iter()
        .filter(|one| !asked.contains(&one.uri))
        .collect();
    let clean = left
        .iter()
        .filter(|one| !named_by_kept.contains(one.uri.as_str()))
        .count();

    Plan {
        kept,
        unknown,
        left: left.len(),
        clean,
        broken,
    }
}

// ---------------------------------------------------------------------------
// Making the cut
// ---------------------------------------------------------------------------

/// What to do with the arrays a kept object points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrays {
    /// Copy the referenced datasets into a file beside the new package.
    ///
    /// The default, and the only one that makes the new package **smaller**:
    /// a partial EPC pointing at the whole source `.h5` is legal and misses the
    /// point entirely.
    ///
    /// The paths inside the new file are the **same** as in the source. Keeping
    /// them means the objects need no rewriting, so an extraction cannot
    /// invalidate a pointer by renumbering one — and a person comparing the two
    /// files reads the same names.
    Copy,
    /// Leave the pointers alone: the new package names the source's data file.
    ///
    /// Legitimate when the data file travels separately, or when there is no
    /// writing backend. It is **said** in the report rather than assumed.
    Leave,
}

/// What an extraction did.
#[derive(Debug, Clone)]
pub struct Extraction {
    /// The plan it carried out.
    pub plan: Plan,
    /// How many datasets were copied.
    pub arrays: usize,
    /// Datasets a kept object points at that **could not be copied**.
    ///
    /// A build with no writing backend, a pointer the source cannot satisfy.
    /// Counted and named rather than skipped: a package written without the
    /// numbers it promises looks exactly like one written with them, and the
    /// only moment anybody would find out is when a viewer draws nothing.
    pub arrays_missed: Vec<String>,
    /// Objects the source holds and could not be read, so could not be written.
    ///
    /// Named, never skipped: a package short of an object nobody mentioned is
    /// the failure this whole feature exists to prevent.
    pub unreadable: Vec<String>,
}

/// The bytes of an EPC holding nothing — the two parts the specification makes
/// mandatory, and no object.
///
/// Built rather than copied from a source: a new package must not inherit
/// content types for parts it will not carry.
///
/// **Public since 08/09/2026**, and the reason is a count. This was the only
/// way in the workspace to make an empty package and it was private, so
/// `energyml-build` — which creates one when no EPC is given — would have
/// needed a second copy. There is already a second: a front ender
/// writes the same two parts in TypeScript, because the front end makes
/// one before any Rust is asked anything. Two implementations of *an EPC is two
/// small XML parts in a zip* is one more than anybody wants; three would be a
/// habit.
pub fn empty_package(created: Option<i64>) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    use std::io::Write;
    use crate::epc::core_props::{CORE_CONTENT_TYPE, CORE_PART, CORE_RELATIONSHIP, CoreProperties, EPC_VERSION};

    // `created` is in Unix seconds. `None` takes the machine's clock, which is what
    // the CLI, the Python bridge and the tests do; a browser **must** give
    // one, its own being the only one within reach.
    //
    // **Three parts, not two.** The first two are OPC's; the third is
    // `docProps/core.xml`, which the EPC makes mandatory (§3.5) — with its
    // content type and the package relationship that §3.5.2 requires.
    //
    // It was missing, and the defect was visible from outside: `EpcStore`
    // warns *"absent, though the EPC conventions make the core properties
    // mandatory"*, so **every new package opened with a health warning**.
    // Two writers in the repository disagreed on what a minimal EPC is — the
    // one that judges and the one that creates — which is the first kind of
    // defect this workspace names.
    let core = CoreProperties {
        // The producer is **this tool**, and that is the truth: nobody has put
        // anything in it yet. A user name would be data the front end does not have
        // and would have to invent.
        creator: "energyml-rust".to_string(),
        // **The clock comes from the caller**, and this is not a preference:
        // `SystemTime::now()` on `wasm32-unknown-unknown` does not return an
        // error, it **panics** — *"time not implemented on this platform"*.
        // It is the rule a front end's save already paid for, and that this empty
        // package reintroduced: creating an EPC in the front end panicked at the
        // very moment the mandatory part was added.
        created: match created {
            Some(seconds) => crate::epc::core_props::w3cdtf(seconds),
            None => crate::epc::core_props::now_w3cdtf(),
        },
        version: EPC_VERSION.to_string(),
        ..CoreProperties::default()
    };

    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Override PartName="/{CORE_PART}" ContentType="{CORE_CONTENT_TYPE}"/>
</Types>"#
    );

    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">
",
    );
    let _ = writeln!(
        rels,
        "  <Relationship Id=\"coreProperties\" Type=\"{CORE_RELATIONSHIP}\" Target=\"{CORE_PART}\"/>"
    );
    rels.push_str("</Relationships>
");

    let mut bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("[Content_Types].xml", options)
            .map_err(|e| Error::Package(format!("writing the content types: {e}")))?;
        zip.write_all(content_types.as_bytes())?;
        zip.start_file("_rels/.rels", options)
            .map_err(|e| Error::Package(format!("writing the package relationships: {e}")))?;
        zip.write_all(rels.as_bytes())?;
        zip.start_file(CORE_PART, options)
            .map_err(|e| Error::Package(format!("writing the core properties: {e}")))?;
        zip.write_all(core.to_xml().as_bytes())?;
        zip.finish()
            .map_err(|e| Error::Package(format!("closing the package: {e}")))?;
    }
    Ok(bytes)
}

/// Write the chosen objects out as a package of their own.
///
/// The order of operations is deliberate: **plan, then write**. A caller that
/// wants to show the consequences first calls [`plan`] and gets the same answer
/// this uses, so what somebody agreed to is what happens.
///
/// `wanted` is expected to be closed — see [`plan`], which reports every
/// reference it does not close. This writes it as given: repairing a selection
/// on the way past would produce a package nobody chose.
pub fn extract(
    source: &EpcStore,
    wanted: &[String],
    out: &std::path::Path,
    arrays: Arrays,
    options: &SaveOptions,
) -> Result<Extraction> {
    let resources = source.resources();
    let mut objects = Vec::new();
    for summary in &resources {
        if let Ok(object) = source.get_object(&summary.uri) {
            objects.push((summary, object));
        }
    }
    let graph = crate::graph::build(objects.iter().map(|(s, o)| (*s, o.as_reflect())));
    let made = plan(&resources, &graph, wanted);

    // The destination is opened as a package before anything is put in it, so
    // the store's external root is the output folder and a copied array lands
    // beside the package rather than beside the source.
    std::fs::write(out, empty_package(None)?)?;
    let mut target = EpcStore::open(out, source.registry_arc())?;

    let filled = fill(source, &mut target, &made.kept)?;
    let mut copied = 0usize;
    let mut missed = Vec::new();
    if arrays == Arrays::Copy {
        for location in &filled.arrays {
            match source.read_array(location) {
                Ok(data) => {
                    target.write_array_to(&location.file, &location.path_in_file, &data)?;
                    copied += 1;
                }
                // A pointer the source cannot satisfy is not the extraction's
                // to invent, and a build with no writing backend cannot copy at
                // all. Either way it is **named** — see `arrays_missed`.
                Err(_) => missed.push(format!("{}:{}", location.file, location.path_in_file)),
            }
        }
    }
    target.save(options)?;

    Ok(Extraction {
        plan: made,
        arrays: copied,
        arrays_missed: missed,
        unreadable: filled.unreadable,
    })
}

/// What [`fill`] put into a target, and what the objects it put point at.
struct Filled {
    /// Kept objects the source could not read, so could not hand over.
    unreadable: Vec<String>,
    /// Every dataset the written objects point at, **once** each, in the order
    /// they were met.
    ///
    /// Once, because two objects naming the same dataset is ordinary — a grid
    /// and its sub-representation — and copying it twice would either fail on
    /// the second write or, worse, count one array as two.
    arrays: Vec<crate::array::ArrayLocation>,
}

/// Put the kept objects into `target`, and declare the files their proxies name.
///
/// The half every way of cutting shares — to a file on disk ([`extract`]) or to
/// bytes in memory ([`to_bytes`]) — so the two cannot disagree about which
/// objects go and which file each proxy stands for. What they do with the
/// arrays differs, and is theirs.
fn fill(source: &EpcStore, target: &mut EpcStore, kept: &[String]) -> Result<Filled> {
    let mut unreadable = Vec::new();
    let mut arrays = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = Default::default();
    // Which external file each proxy names. The new package has to **declare**
    // them, or its `.rels` cannot point at the proxy — and `energyml check`
    // then reports a reference the objects make and the index omits. Found by
    // running the command and reading the check, not by reasoning about it.
    let mut files: std::collections::BTreeMap<String, String> = Default::default();
    for uri in kept {
        let object = match source.get_object(uri) {
            Ok(object) => object,
            Err(_) => {
                unreadable.push(uri.clone());
                continue;
            }
        };
        for location in source.arrays_of(object.as_reflect()) {
            // An unresolved proxy has no file to declare: registering an empty
            // name would write a relationship that points nowhere.
            if let Some(proxy) = location.proxy_uuid.as_ref().filter(|_| !location.file.is_empty()) {
                files.insert(proxy.to_ascii_lowercase(), location.file.clone());
            }
            if seen.insert((location.file.clone(), location.path_in_file.clone())) {
                arrays.push(location);
            }
        }
        target.put(object)?;
    }
    // Declared **before** the save, because the save is what writes the `.rels`
    // that name them.
    for (proxy, file) in &files {
        target.register_external_file(proxy, file.clone());
    }
    Ok(Filled { unreadable, arrays })
}

// ---------------------------------------------------------------------------
// Cutting to bytes, for a caller with no disk
// ---------------------------------------------------------------------------

/// One data file the cut package names, and the name it will carry there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFile {
    /// The file as the source names it — its bare name.
    pub from: String,
    /// The name the cut package will give it. Equal to `from` when nobody
    /// asked for another.
    pub to: String,
    /// How many datasets of it the kept objects point at.
    pub arrays: usize,
}

/// One dataset to carry into the cut's data file.
///
/// The **path does not change** — the same bargain as [`Arrays::Copy`]: keeping
/// it means no object has to be rewritten for its numbers, only for the name of
/// the file that holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayCopy {
    /// The file the source reads it from, as the kept object declares it.
    pub from: String,
    /// The dataset inside both files.
    pub path: String,
    /// The file of the cut package it goes into.
    pub to: String,
}

/// What cutting a package to bytes would come to — or came to.
///
/// [`Plan`] plus the two things a caller without a disk has to know: which data
/// files the new package names and under which names, and which datasets must
/// travel for its pointers to be satisfied.
#[derive(Debug, Clone, Default)]
pub struct Cut {
    /// The plan, as [`plan`] makes it.
    pub plan: Plan,
    /// Every data file the kept objects point at, in the order first met.
    pub files: Vec<DataFile>,
    /// Every dataset to carry, once each.
    ///
    /// **The caller copies them.** Writing HDF5 is a backend's job, and the
    /// caller this exists for — a browser — holds its files in h5wasm, beyond
    /// this crate's reach. The same division `create_apply` makes.
    pub arrays: Vec<ArrayCopy>,
    /// Array pointers whose file nothing names — a proxy with no `.rels`.
    ///
    /// Named, never dropped: the package will point at them and nobody can
    /// copy them, so the numbers will be missing and somebody must be told.
    pub unnamed: Vec<String>,
    /// Kept objects the source could not read, so could not write.
    pub unreadable: Vec<String>,
    /// Fields rewritten to carry a new file name (eml 2.3 `Uri`).
    pub fields: usize,
    /// Relationships rewritten to carry a new file name (EPC §3.2.3).
    pub relationships: usize,
    /// Why this cut will not be written, in words a person can act on.
    pub refusals: Vec<String>,
}

impl Cut {
    /// Whether the cut can be written.
    pub fn is_allowed(&self) -> bool {
        self.refusals.is_empty()
    }
}

/// What cutting `source` to `wanted` would do, renaming its data files as
/// `names` says — without producing a byte.
///
/// `names` maps a data file's bare name, as [`Cut::files`] lists it, to the
/// name it should carry in the new package. A file left out keeps its name.
///
/// The quote is computed by **doing the cut in memory** and not serialising it:
/// the rename's refusals are only known once there is a package to rename in,
/// and a quote that computed them another way would be a second answer to the
/// question [`to_bytes`] answers.
pub fn quote(source: &EpcStore, wanted: &[String], names: &std::collections::BTreeMap<String, String>) -> Result<Cut> {
    // **A fixed date, never the machine's clock.** A quote's package is never
    // written, so the date it would carry is nobody's concern — while
    // `SystemTime::now()` **panics** on `wasm32-unknown-unknown` (*"time not
    // implemented on this platform"*). Passing `None` here made the front end's
    // export dialog panic on its first quote: the rule
    // `empty_package` documents, broken one call above it.
    prepare(source, wanted, names, Some(0)).map(|(cut, _)| cut)
}

/// Cut `source` to `wanted`, as bytes: the package, and the list of datasets
/// the caller must copy into the renamed data files.
///
/// Refused as a whole while [`quote`] has a refusal — a cut that breaks what
/// was whole, a rename the package cannot carry. `created` is the clock, in
/// Unix seconds, which a browser must supply (`SystemTime::now` panics there).
pub fn to_bytes(
    source: &EpcStore,
    wanted: &[String],
    names: &std::collections::BTreeMap<String, String>,
    created: Option<i64>,
) -> Result<(Cut, Vec<u8>)> {
    let (cut, target) = prepare(source, wanted, names, created)?;
    if !cut.is_allowed() {
        return Err(Error::Package(format!(
            "this cut was refused: {}",
            cut.refusals.join("; ")
        )));
    }
    let mut options = SaveOptions::default();
    if let Some(seconds) = created {
        options.created = Some(crate::epc::core_props::w3cdtf(seconds));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    target.write_to(&mut out, &options)?;
    Ok((cut, out.into_inner()))
}

/// The cut, made in memory: the package it would write, and what it would say.
fn prepare(
    source: &EpcStore,
    wanted: &[String],
    names: &std::collections::BTreeMap<String, String>,
    created: Option<i64>,
) -> Result<(Cut, EpcStore)> {
    let resources = source.resources();
    let (held, _) = source.read_objects(None);
    let graph = crate::graph::build(held.iter().map(|(summary, object)| (summary, object.as_reflect())));
    let made = plan(&resources, &graph, wanted);

    let blank = empty_package(created)?;
    let mut target = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(blank)),
        "selection.epc",
        source.registry_arc(),
    )?;
    let filled = fill(source, &mut target, &made.kept)?;

    let mut cut = Cut {
        unreadable: filled.unreadable,
        ..Cut::default()
    };

    // The files, by bare name: `./data/model.h5` and `model.h5` are one file to
    // the rename (`rename::names_file`), so they are one file here too.
    for location in &filled.arrays {
        if location.file.is_empty() {
            cut.unnamed.push(format!("{} ({})", location.path_in_file, location.field_path));
            continue;
        }
        let bare = bare_name(&location.file);
        match cut.files.iter_mut().find(|one| one.from == bare) {
            Some(file) => file.arrays += 1,
            None => cut.files.push(DataFile {
                from: bare.to_string(),
                to: names.get(bare).cloned().unwrap_or_else(|| bare.to_string()),
                arrays: 1,
            }),
        }
    }

    if made.kept.is_empty() {
        cut.refusals
            .push("none of the chosen objects is in this package, so there is nothing to write".to_string());
    }
    let breaks = made.broken.iter().filter(|one| one.in_source).count();
    if breaks > 0 {
        cut.refusals.push(format!(
            "this selection is not closed: {breaks} reference(s) would be broken — add what they name"
        ));
    }
    for asked in names.keys() {
        if !cut.files.iter().any(|one| &one.from == asked) {
            let named: Vec<&str> = cut.files.iter().map(|one| one.from.as_str()).collect();
            cut.refusals.push(format!(
                "`{asked}` is not a data file the kept objects name — they name {}",
                if named.is_empty() { "none".to_string() } else { named.join(", ") }
            ));
        }
    }

    // The new package has no disk, and saying so is what makes the rename stop
    // at the package: its files are the caller's to write.
    for file in &cut.files {
        target.external_files_mut().declare_present(file.from.clone());
    }
    // **The rename is the rename operation**, run on the cut package: the same
    // three places, the same refusals — a second way of rewriting a file name
    // would be a second answer to where file names are written.
    for file in cut.files.clone() {
        if file.to == file.from {
            continue;
        }
        let asked = crate::rename::plan(&target, &file.from, &file.to);
        if !asked.is_allowed() {
            cut.refusals.extend(
                asked
                    .refusals
                    .iter()
                    .map(|why| format!("renaming `{}` to `{}`: {why}", file.from, file.to)),
            );
            continue;
        }
        let done = crate::rename::apply(&mut target, &asked)?;
        cut.fields += done.fields;
        cut.relationships += done.relationships;
    }

    cut.arrays = filled
        .arrays
        .iter()
        .filter(|location| !location.file.is_empty())
        .map(|location| {
            let bare = bare_name(&location.file);
            let to = cut
                .files
                .iter()
                .find(|one| one.from == bare)
                .map(|one| one.to.clone())
                .unwrap_or_else(|| bare.to_string());
            ArrayCopy {
                from: location.file.clone(),
                path: location.path_in_file.clone(),
                to,
            }
        })
        .collect();
    cut.plan = made;
    Ok((cut, target))
}

/// The last segment of a declared location — how the rename compares names.
fn bare_name(declared: &str) -> &str {
    match declared.rfind(['/', '\\']) {
        Some(at) => &declared[at + 1..],
        None => declared,
    }
}

// ---------------------------------------------------------------------------
// Copying between packages
// ---------------------------------------------------------------------------

/// What a copy from one package into another did.
#[derive(Debug, Clone, Default)]
pub struct Copied {
    /// Every object written into the target, in the source's order.
    pub objects: Vec<String>,
    /// Of those, the ones **nobody asked for**: what the closure pulled in.
    ///
    /// Named rather than folded into the total, because it is the news. A
    /// caller asking for one surface and receiving six objects should be told
    /// which five came along and not left to diff two lists.
    pub added: Vec<String>,
    /// Referenced by what was copied, and the source does not hold it.
    ///
    /// The copy is still made — refusing would make a package that is already
    /// broken uncopyable — but the target inherits the dangling reference and
    /// the caller has to know. Concepts the standard publishes in a catalogue
    /// are not in here.
    pub missing: Vec<String>,
    /// How many datasets were copied.
    pub arrays: usize,
    /// Datasets a copied object points at that could not be copied.
    pub arrays_missed: Vec<String>,
    /// Objects the source could not deserialize, so could not hand over.
    pub unreadable: Vec<String>,
}

/// Copy objects — **and everything they need** — from one package into another.
///
/// What `copy_part_from_other_model` is in resqpy, and the operation a partial
/// export cannot stand in for: the target already exists and keeps what it has.
///
/// The set is **closed here** ([`crate::graph::closure`]): asking for a
/// representation brings its interpretation, the feature behind it and the CRS
/// they hang off, because a representation without them is an object that
/// opens and means nothing. What the closure added is reported, so the caller
/// sees what it received.
///
/// Objects already in the target are written over: a uuid is an identity, and
/// two packages holding the same uuid hold the same object. The alternative —
/// renumbering on the way in — would break every reference that names it.
pub fn copy_into(
    from: &EpcStore,
    into: &mut EpcStore,
    wanted: &[String],
    arrays: Arrays,
) -> Result<Copied> {
    let (held, unreadable) = from.read_objects(None);
    let graph = crate::graph::build(held.iter().map(|(summary, object)| (summary, object.as_reflect())));
    let closed = crate::graph::closure(&graph, wanted);

    let asked: std::collections::HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let mut done = Copied {
        added: closed
            .needed
            .iter()
            .filter(|uri| !asked.contains(uri.as_str()))
            .cloned()
            .collect(),
        missing: closed.missing.clone(),
        unreadable,
        ..Default::default()
    };

    // Which external file each proxy names, for the target's own `.rels` —
    // the same bargain `extract` strikes, and found the same way: by running
    // the command and reading `energyml check` afterwards.
    let mut files: std::collections::BTreeMap<String, String> = Default::default();
    for uri in &closed.needed {
        let object = match from.get_object(uri) {
            Ok(object) => object,
            Err(_) => {
                done.unreadable.push(uri.clone());
                continue;
            }
        };
        for location in from.arrays_of(object.as_reflect()) {
            if let Some(proxy) = location.proxy_uuid.clone() {
                files.insert(proxy.to_ascii_lowercase(), location.file.clone());
            }
            if arrays != Arrays::Copy {
                continue;
            }
            match from.read_array(&location) {
                Ok(data) => {
                    into.write_array_to(&location.file, &location.path_in_file, &data)?;
                    done.arrays += 1;
                }
                // A pointer the source cannot satisfy is not the copy's to
                // invent, and a build with no writing backend cannot copy at
                // all. Either way it is named.
                Err(_) => done
                    .arrays_missed
                    .push(format!("{}:{}", location.file, location.path_in_file)),
            }
        }
        into.put(object)?;
        done.objects.push(uri.clone());
    }
    for (proxy, file) in &files {
        into.register_external_file(proxy, file.clone());
    }
    Ok(done)
}
