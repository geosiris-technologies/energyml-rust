//! The package: open one or make one, put the objects in, write the numbers.
//!
//! The only module of this crate that knows a store exists. Everything it
//! writes has been decided already — [`crate::assemble`] grouped and
//! deduplicated, [`crate::build`] and [`crate::table`] made the objects — so
//! what is left here is plumbing, and plumbing is where an honest error message
//! matters more than a clever one.
//!
//! # An EPC in, or a new one
//!
//! The rule: **take an EPC when one is given, create one when none is.**
//! Adding to an existing package is the ordinary case — a pipeline's own
//! packages already hold the stages these units belong to,
//! and importing into them keeps one model rather than making a second.
//!
//! # Order of writing, and why it is not free
//!
//! References are written before their targets exist in the archive but never
//! before their targets exist *in memory*, and the `.rels` are regenerated from
//! the objects at save time — so the order below is about what a **reader** of a
//! half-written package would see, not about correctness. It still matters: a
//! failure part-way through leaves a package on disk, and one whose CRS is there
//! and whose points are not is easier to understand than the reverse.
//!
//! 1. the CRS objects — nothing points at them yet;
//! 2. the table — likewise;
//! 3. per group: the feature, its interpretation, its point set, its property;
//! 4. the coordinates, into the external file;
//! 5. the canonical `ActivityTemplate`, unless the package already holds it;
//! 6. the activity, which names everything above.

use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_eml_2_3 as eml;
use energyml_storage::EpcStore;

use crate::assemble::Plan;
use crate::crs::{self, Vertical};

/// What an import needs to know that no file says.
#[derive(Debug, Clone)]
pub struct Options {
    /// The name the package will carry.
    pub name: String,
    /// The external file the coordinates go into.
    pub data_file: String,
    /// Which vertical system the elevations are in.
    pub vertical: Vertical,
    /// What the coordinates are written in: the degrees as they stand, or metres
    /// under a projected system.
    ///
    /// Degrees are lossless and unusable in a 3D view — the horizontal extent is
    /// in degrees and the vertical in metres, and nothing in the file lets a
    /// reader reconcile them. Projecting moves the numbers, so the target is the
    /// caller's and never guessed. See [`crate::crs`].
    pub target: crs::Target,
    /// The EPSG code the file's coordinates are **in**. A GeoJSON is 4326 by
    /// definition (RFC 7946); the field exists so a caller reading something
    /// else can say so rather than have this module assume.
    pub source_epsg: i64,
    /// `YYYY-MM-DDThh:mm:ssZ` — the run's own instant.
    ///
    /// **From the caller**: a clock is an input, and a build that reads one
    /// cannot be replayed. Everything derived from it — the citations, the
    /// activity's uuid — is then reproducible.
    pub creation: String,
    /// Who is writing. Goes into every `Citation`.
    pub originator: String,
    /// The source file's name, for the activity and the titles.
    pub source: String,
    /// The stage the export was asked about.
    pub stage: String,
}

impl Options {
    /// A citation stamped with this run's originator and instant.
    pub fn citation(&self, title: &str) -> eml::Citation {
        eml::Citation {
            title: title.to_string(),
            originator: self.originator.clone(),
            creation: self.creation.clone(),
            // The convention the specification asks for:
            // `organisation:application:version/format`.
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        }
    }
}

/// What an import wrote.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// The uris of every object put in the package.
    pub written: Vec<String>,
    /// Distinct places written.
    pub points: usize,
    /// Source features they came from.
    pub features: usize,
    /// What the plan did not take, each with its reason.
    pub skipped: Vec<String>,
    /// What could not be written and why — **never a count on its own**.
    pub refused: Vec<String>,
    /// What the plan says of what it wrote, which is **not** a failure — the
    /// named exception of degrees (`frame-degrees`) is one. Kept apart
    /// from `refused`, which a caller reads as a failure: `energyml import`
    /// fails on a refusal, and an import in degrees — the default — is not one.
    pub notes: Vec<String>,
}

impl Report {
    /// How many source features the deduplication folded away.
    pub fn duplicates(&self) -> usize {
        self.features.saturating_sub(self.points)
    }
}

/// Open the package to import into, or make an empty one.
///
/// `None` is not an error and not a default hidden in a signature: it is the
/// case where no package is given, and it produces a package with nothing in it but the two
/// parts the specification requires.
pub fn open(
    source: Option<&std::path::Path>,
    name: &str,
    registry: Arc<Registry>,
) -> energyml_storage::Result<EpcStore> {
    let _span = tracing::info_span!("build.open_package", name, from_file = source.is_some()).entered();
    match source {
        Some(path) => EpcStore::open(path, registry),
        None => EpcStore::from_reader(
            Box::new(std::io::Cursor::new(energyml_storage::extract::empty_package(
                None,
            )?)),
            name,
            registry,
        ),
    }
}

/// The plan's notes that report on a write rather than a failure.
const INFORMATIVE: &[&str] = &["frame-degrees"];

/// Write a plan into a store.
///
/// Does **not** save: the caller decides where the package goes, and a function
/// that both built and saved would give a caller no way to look before writing.
pub fn write(store: &mut EpcStore, plan: &Plan, options: &Options) -> Report {
    let _span =
        tracing::info_span!("build.write", points = plan.points(), features = plan.features()).entered();
    let mut report = Report {
        skipped: plan.skipped(),
        points: plan.points(),
        features: plan.features(),
        ..Default::default()
    };
    // The objects are decided by the operation, the writing is the generic
    // one: this function is the SIGEOL door onto both, kept so that `energyml
    // import` and the corpus tests read exactly as before.
    let planned = match crate::ops::sigeol::plan_of(plan, options) {
        Ok(planned) => planned,
        Err(why) => {
            report.refused.push(why.message());
            return report;
        }
    };
    // A note is a refusal unless it says something about a write that
    // happened: `not-placed` is a point that reached no file, `frame-degrees`
    // the frame the point sets were written under.
    for note in &planned.says {
        if INFORMATIVE.contains(&note.key) {
            report.notes.push(note.detail.clone());
        } else {
            report.refused.push(note.detail.clone());
        }
    }
    let made = crate::apply::apply(
        &planned,
        store,
        &crate::operation::When {
            creation: options.creation.clone(),
            originator: options.originator.clone(),
        },
    );
    report.written = made.written;
    report.refused.extend(made.refused);
    report
}
