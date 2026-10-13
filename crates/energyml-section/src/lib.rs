//! The section view - long and cross sections of a model along a trace.
//!
//! ADR-0015 is this crate's specification: every
//! non-trivial choice in the code follows the decision recorded there.
//!
//! # Three stages, three costs
//!
//! ```text
//! inputs ──► read (per package) ──► assemble ──► Catalogue      expensive, cached
//!                                                 │
//!                                       View ──► layout ──► Layout ──► svg::write
//! ```
//!
//! 1. [`read::contribution`] reads one package's objects - wells, surfaces,
//!    grids - reprojected into the scene's system and cut along every section
//!    trace. It is the only stage that touches a store.
//! 2. [`assemble::assemble`] joins the contributions of every package into a
//!    [`model::Catalogue`]: wells projected on each section, frames and
//!    trajectories of one wellbore folded into one line, every object's fate.
//! 3. [`layout::layout`] turns a catalogue and a [`model::View`] into a
//!    [`layout::Layout`] - scales, ticks, fills, labels, all in numbers - and
//!    [`svg::write`] writes it. Nothing is read or recomputed here: the front end
//!    redraws on every view change, the export draws the same view.
//!
//! The same split as `energyml-sheet` (`Board` → `layout` → `svg::write`), the
//! precedent of a drawing computed in Rust.
//!
//! # No native dependency
//!
//! wasm is a hard requirement (see ADR-0007): everything here is
//! computation over a `ValueSource`, and the SVG is a `String`.

pub mod assemble;
pub mod crosses;
pub mod erosion;
pub mod inputs;
pub mod layout;
pub mod markers;
pub mod model;
pub mod paint;
pub mod panels;
pub mod png;
pub mod read;
pub mod spatial;
pub mod style;
pub mod svg;

use crate::inputs::InputsError;
use crate::model::{Asked, Catalogue, Contribution, Inputs, SectionTrace, View};

/// The whole calculation for one call: the sections' traces, one contribution
/// per package - read by `read_one`, which is the face's, since only the face
/// holds the packages - and the catalogue.
///
/// The faces (the CLI, wasm) call this rather than chaining the stages
/// themselves: no face carries logic (ADR-0012).
pub fn compute(
    inputs: &Inputs,
    mut read_one: impl FnMut(&Asked, &[SectionTrace]) -> Contribution,
) -> Result<Catalogue, InputsError> {
    let (sections, _) = crosses::traces(inputs)?;
    let contributions: Vec<Contribution> = inputs
        .objects
        .iter()
        .map(|asked| read_one(asked, &sections))
        .collect();
    assemble::assemble(inputs, &contributions)
}

/// One package handed to [`compute_packages`]: the store id its objects are
/// asked under (`Asked::store`), where they are read, what the face already
/// knows of it, and - when the face keeps one - its cache.
pub struct Opened<'a> {
    /// The id `Asked::store` names it by (`s1`, `s2`, … in the front end).
    pub store: String,
    /// Where its objects and arrays are read.
    pub source: &'a dyn energyml_domain::ArraySource,
    /// Its colour sets, index and colour resolver - the face's own when it has them.
    pub context: read::PackageContext<'a>,
    /// What it keeps between two calculations; `None`: nothing is kept.
    pub cache: Option<&'a mut read::PackageCache>,
}

/// The whole calculation over **one or several packages**, each with its own
/// selection (`Inputs::objects`, one `Asked` per package): every package read
/// through its cache, the contributions merged by [`assemble::assemble`] into
/// one catalogue - the wells of all packages projected on the same sections,
/// their surfaces cut along the same traces, in the scene's one system.
///
/// A selection naming a package that is not among `packages` is not dropped:
/// each of its objects comes back refused, naming the package.
///
/// A face that holds its packages in separate owners - the front end has one wasm
/// `Session` per package - cannot hand them in one call: it calls
/// [`crosses::traces`], then [`read::contribution_cached`] in each, then
/// [`assemble::assemble`]. That is this function, cut where its owners are.
pub fn compute_packages(
    inputs: &Inputs,
    packages: &mut [Opened<'_>],
    reprojector: &dyn energyml_domain::reproject::Reprojector,
) -> Result<Catalogue, InputsError> {
    let scene = read::Scene {
        system: inputs.system.as_ref(),
        time_factor: inputs.time_factor,
        reprojector,
    };
    compute(inputs, |asked, sections| {
        match packages.iter_mut().find(|one| one.store == asked.store) {
            Some(opened) => {
                let mut thrown = read::PackageCache::new();
                let cache = opened.cache.as_deref_mut().unwrap_or(&mut thrown);
                read::contribution_cached(opened.source, asked, sections, &scene, &opened.context, cache)
            }
            None => not_open(asked),
        }
    })
}

/// Every object of a selection whose package is not open, refused by name.
fn not_open(asked: &Asked) -> Contribution {
    let refused = asked
        .wells
        .iter()
        .chain(&asked.surfaces)
        .map(|uri| model::Refused {
            object: model::ObjectId {
                store: asked.store.clone(),
                uri: uri.clone(),
                uuid: read::surfaces::uuid_of_uri(uri),
                title: String::new(),
                qualified_type: String::new(),
            },
            reason: format!(
                "the package `{}` this object was asked from is not open",
                asked.store
            ),
        })
        .collect();
    Contribution {
        store: asked.store.clone(),
        refused,
        ..Contribution::default()
    }
}

/// Why a drawing could not be made again.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RedrawError {
    /// The SVG's metadata does not read.
    #[error(transparent)]
    Metadata(#[from] svg::MetadataError),
    /// What the metadata says cannot be computed.
    #[error(transparent)]
    Inputs(#[from] InputsError),
}

/// **A drawing made again** from what its own metadata says it was made of -
/// the inputs and the view - over the packages given (`--from-svg`).
///
/// On the same packages, the same bytes: that is how a drawing kept beside a
/// model is known to be the model's still.
pub fn redraw(
    svg: &str,
    packages: &mut [Opened<'_>],
    reprojector: &dyn energyml_domain::reproject::Reprojector,
) -> Result<String, RedrawError> {
    let metadata = svg::read_metadata(svg)?;
    let catalogue = compute_packages(&metadata.inputs, packages, reprojector)?;
    Ok(draw(&catalogue, &metadata.view))
}

/// A catalogue drawn for a view: [`layout::layout`] then [`svg::write`] - what
/// the screen, the download, the CLI and the package all call (one
/// drawer, so the export is what was shown).
pub fn draw(catalogue: &Catalogue, view: &View) -> String {
    svg::write(&layout::layout(catalogue, view))
}
