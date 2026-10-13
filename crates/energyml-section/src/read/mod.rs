//! What one package gives the calculation (ADR-0015).
//!
//! The only stage that touches a store. Everything leaves here **in the scene's
//! system** - reprojected when the object's own differs - and a
//! surface in time leaves carried onto the depth axis by the scene's time
//! factor, so [`crate::assemble`] needs no source.
//!
//! Two halves, two owners: [`surfaces`] and [`wells`].
//! This file only loops; neither half decides anything for the other.

pub mod cache;
pub mod finite;
pub mod frame;
pub mod grids;
pub mod package;
pub mod surfaces;
pub mod wells;

pub use cache::{PackageCache, Work};

use std::collections::HashSet;

use crate::read::cache::HeadSlot;
use crate::read::wells::HeadRead;
use crate::spatial::Rect;

use energyml_domain::ArraySource as ValueSource;
use energyml_domain::reproject::Reprojector;

use energyml_core::registry::DynObject;
use energyml_domain::color::GraphicalSet;
use energyml_domain::package_index::PackageIndex;

use crate::model::{Asked, Contribution, SectionTrace, System};

/// The frames that may represent a wellbore, by its lowercase uuid.
pub type FramesOf<'a> = dyn Fn(&str) -> Vec<Box<dyn DynObject>> + 'a;

/// The colour the file gives an object, by its **full uri** (version included
/// when it declares one - two versions are two objects), as a face that has
/// already resolved it answers - `None` when the file gives none.
pub type ColourOf<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// What the package says that a [`ValueSource`] cannot be asked for - each
/// needs a listing of the package, which only the face holding it can make.
#[derive(Default)]
pub struct PackageContext<'a> {
    /// The package's `GraphicalInformationSet`s, read, each with its uuid -
    /// where the colour of a stratigraphic unit and of a surface come from.
    /// Empty: nothing has a colour from the file.
    pub graphics: &'a [(String, GraphicalSet)],
    /// The frames that may represent a wellbore (a lowercase uuid) -
    /// `strata_by_md::of_representation`'s `frames_of`, asked only for an
    /// object that states no unit itself. `None`: a silent object is not
    /// painted from its sisters.
    pub frames_of: Option<&'a FramesOf<'a>>,
    /// The package's index - with it, a surface's colour follows the whole
    /// inheritance order (collections, models), the one the 3D viewer's ladder
    /// walks (`color_inherit`). `None`: the representation, its interpretation
    /// and its feature only.
    pub index: Option<&'a PackageIndex>,
    /// The face's own resolver of the file's colours, when it has one in cache -
    /// the front end holds every drawn object's ladder already (the front end can pass
    /// its resolver to the calculation so that the search is not redone). Given, it is asked **instead of** the
    /// cascade: no set is merged, no walk is made. It answers the **file's**
    /// colour (the ladder's), never a colour chosen for the scene - those travel
    /// in the view (`View::colours`).
    pub colour_of: Option<&'a ColourOf<'a>>,
}

/// What reading needs to know of the scene, beyond the sections.
pub struct Scene<'a> {
    /// The scene's system - what every point is reprojected into. `None`: a
    /// scene with none; an object that states one is then refused by name.
    pub system: Option<&'a System>,
    /// The scene's time scale (`model::Inputs::time_factor`).
    pub time_factor: Option<f64>,
    /// The geodesy backend for a reprojection (`energyml-proj` in the faces).
    pub reprojector: &'a dyn Reprojector,
}

/// How much a section's windows are grown beyond its widest side, as a share -
/// the slack that keeps rounding from dropping a head that sits exactly on the
/// corridor's edge. Zero would be exact on paper and wrong on a float.
pub const CORRIDOR_SLACK: f64 = 1e-9;

/// The windows a head must fall in to be read: for every segment of every
/// section, its bounding box grown by the section's widest side (plus
/// [`CORRIDOR_SLACK`]) - a superset of the region `Corridor::project` keeps,
/// refuses for a reason, or is within reach of: a head that classifies is
/// within `max(left, right)` of a segment or of an end. `None` when there is
/// no section to bound by.
pub fn corridor_windows(sections: &[SectionTrace]) -> Option<Vec<Rect>> {
    let mut windows = Vec::new();
    for st in sections {
        let reach = st.left.max(st.right);
        let margin = reach * (1.0 + CORRIDOR_SLACK) + 1e-6;
        if !margin.is_finite() {
            // A width that is not a number: `assemble` will refuse the section;
            // reading everything is the answer that does not guess.
            return None;
        }
        for pair in st.trace.windows(2) {
            windows.push(Rect::new(pair[0], pair[1])?.grown(margin));
        }
    }
    (!windows.is_empty()).then_some(windows)
}

/// One package's contribution: its wells read, its surfaces and grids cut
/// along every section, and what could not be read.
///
/// One object that fails never fails the call: it becomes a
/// [`crate::model::Refused`].
///
/// `well_context` is what a `ValueSource` cannot be asked for - the package's colour
/// sets and the frames of a wellbore, both needing a listing of the package -
/// which the face that holds the package supplies (`read::wells`). Its default
/// gives units no colour and does not paint a silent frame from its sisters.
pub fn contribution(
    source: &dyn ValueSource,
    asked: &Asked,
    sections: &[SectionTrace],
    scene: &Scene<'_>,
    well_context: &PackageContext<'_>,
) -> Contribution {
    contribution_cached(
        source,
        asked,
        sections,
        scene,
        well_context,
        &mut PackageCache::new(),
    )
}

/// [`contribution`], answered from what `cache` kept of this package where it
/// is still exact (`cache` explains when) - the one implementation: the
/// uncached call is this one with a cache thrown away.
// The closures return a refusal, which carries the object's whole identity on
// purpose (as `read_surface` says).
#[allow(clippy::result_large_err)]
pub fn contribution_cached(
    source: &dyn ValueSource,
    asked: &Asked,
    sections: &[SectionTrace],
    scene: &Scene<'_>,
    well_context: &PackageContext<'_>,
    cache: &mut PackageCache,
) -> Contribution {
    contribution_bounded(
        source,
        asked,
        sections,
        scene,
        well_context,
        cache,
        Bound::Corridor,
    )
}

/// Which wells of a selection are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// Only the wells whose head is near a section (the default): the others
    /// are told by their head, read from no array, and `assemble` refuses them
    /// as too far exactly as it would have after a full read.
    Corridor,
    /// Every asked well, in full - what the corridor's answer is checked against.
    Everything,
}

/// [`contribution_cached`], saying how far the wells are read ([`Bound`]).
#[allow(clippy::result_large_err, clippy::too_many_arguments)]
pub fn contribution_bounded(
    source: &dyn ValueSource,
    asked: &Asked,
    sections: &[SectionTrace],
    scene: &Scene<'_>,
    well_context: &PackageContext<'_>,
    cache: &mut PackageCache,
    bound: Bound,
) -> Contribution {
    cache.hold_for(&asked.store, asked.revision, scene);
    let mut out = Contribution {
        store: asked.store.clone(),
        ..Contribution::default()
    };
    let read_in_full = |cache: &mut PackageCache, uri: &str, out: &mut Contribution| {
        // Nothing that is not finite leaves here (`finite`): a `NaN` becomes a
        // `null` in JSON, and the catalogue refused every package for it.
        match cache
            .well(uri, || {
                wells::read_well_with(source, &asked.store, uri, scene, well_context)
            })
            .and_then(finite::finite_well)
        {
            Ok((well, note)) => {
                out.wells.push(well);
                out.notes.extend(note);
            }
            Err(refused) => out.refused.push(refused),
        }
    };
    match corridor_windows(sections).filter(|_| bound == Bound::Corridor && !asked.wells.is_empty()) {
        // No section, nothing to bound by: every asked well is read.
        None => {
            for uri in &asked.wells {
                read_in_full(cache, uri, &mut out);
            }
        }
        Some(windows) => {
            // The heads first (no array), then only the wells whose head is near
            // a section.
            let slots: Vec<Option<(usize, HeadRead)>> = asked
                .wells
                .iter()
                .map(|uri| {
                    match cache.head(uri, || wells::read_head(source, &asked.store, uri, scene).ok()) {
                        HeadSlot::Head { at, head } => Some((*at, (**head).clone())),
                        HeadSlot::Unusable => None,
                    }
                })
                .collect();
            let near = cache.heads_in(&windows);
            // A wellbore with a member near a section is read whole, so that
            // the fold ranks all its members as it always did. One whose members
            // are **all** far is neither read nor folded: each member is a
            // head-only stub, refused on its own row as too far (no
            // merging far away). The catalogue's drawn part is
            // the same; the rows of far wells lose their `Merged`, and a far
            // well whose full read would have failed (or noted a dropped node)
            // now says "farther than" instead - both accepted.
            let near_wellbores: HashSet<&str> = slots
                .iter()
                .flatten()
                .filter(|(at, _)| near.contains(at))
                .filter_map(|(_, head)| head.wellbore.as_deref())
                .collect();
            for (uri, slot) in asked.wells.iter().zip(&slots) {
                match slot {
                    Some((at, head))
                        if !near.contains(at)
                            && !head
                                .wellbore
                                .as_deref()
                                .is_some_and(|wellbore| near_wellbores.contains(wellbore)) =>
                    {
                        out.wells.push(head.stub());
                    }
                    _ => read_in_full(cache, uri, &mut out),
                }
            }
        }
    }
    // Every set of the package, merged once for all its surfaces - the one
    // reading of several (`GraphicalSet::merged`, as a front end's graphical-set reader).
    // Not when the face resolves colours itself: nothing would read the sets;
    // and only when a surface is read at all.
    let mut colours: Option<Option<GraphicalSet>> = None;
    for uri in &asked.surfaces {
        let read = cache.surface(uri, sections, || {
            let set = colours
                .get_or_insert_with(|| {
                    (well_context.colour_of.is_none() && !well_context.graphics.is_empty()).then(|| {
                        GraphicalSet::merged(well_context.graphics.iter().map(|(_, set)| set.clone()))
                    })
                })
                .as_ref();
            let colouring = surfaces::Colouring {
                set,
                index: well_context.index,
                resolver: well_context.colour_of,
            };
            surfaces::prepare_surface(source, &asked.store, uri, scene, colouring)
        });
        match read {
            Ok(mut surface) => {
                finite::finite_surface(&mut surface);
                out.surfaces.push(surface);
            }
            Err(refused) => out.refused.push(refused),
        }
    }
    out
}
