//! What a face that holds a whole package — an `EpcStore` — gives the section
//! besides its objects: its colour sets, its index, and the objects a section
//! takes when nobody chose.
//!
//! This was written in `energyml profile`, and a second face would have written
//! it again (ADR-0012: no face carries logic). A face that
//! already holds these — the front end keeps its index and its colour walks — builds
//! a [`crate::read::PackageContext`] from its own instead, and never calls
//! [`Listed::of`].

use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::color::GraphicalSet;
use energyml_domain::package_index::PackageIndex;
use energyml_storage::EpcStore;

use crate::read::surfaces::is_cut_type;
use crate::read::{FramesOf, PackageContext};

/// A package's listing, read once: its colour sets, its index, and the frames
/// of a wellbore by that index.
pub struct Listed<'a> {
    /// Every `GraphicalInformationSet` of the package, read, with its uuid.
    pub graphics: Vec<(String, GraphicalSet)>,
    /// The package's index, for the inheritance order of colours.
    pub index: Arc<PackageIndex>,
    frames_of: Box<FramesOf<'a>>,
}

impl<'a> Listed<'a> {
    /// Read the listing of `store`. A colour set that does not read is refused
    /// by its uri: drawing without it would draw the file's colours as absent.
    pub fn of(store: &'a EpcStore) -> Result<Self, String> {
        let mut graphics = Vec::new();
        for one in store
            .resources()
            .iter()
            .filter(|one| one.qualified_type.ends_with("GraphicalInformationSet"))
        {
            let object = store
                .get_object(&one.uri)
                .map_err(|why| format!("the colour set `{}` does not read: {why}", one.uri))?;
            graphics.push((
                one.uuid.to_ascii_lowercase(),
                GraphicalSet::read(object.as_reflect()),
            ));
        }
        let index = Arc::new(PackageIndex::build(store, store.revision()));
        let held = Arc::clone(&index);
        let frames_of: Box<FramesOf<'a>> =
            Box::new(move |wellbore: &str| frames_of_wellbore(store, &held, wellbore));
        Ok(Self {
            graphics,
            index,
            frames_of,
        })
    }

    /// The context the calculation reads through — borrowed from this listing.
    /// The same the front end's session gives: its index, and a silent well painted
    /// from the frames of its wellbore.
    pub fn context(&self) -> PackageContext<'_> {
        PackageContext {
            graphics: &self.graphics,
            frames_of: Some(&*self.frames_of),
            index: Some(&self.index),
            colour_of: None,
        }
    }
}

/// The frames that represent a wellbore (a uuid), **by the index** — its
/// dependents of that type, never a scan of the package (measured on a front end's
/// `frames_of_wellbore`: 1.15 s a well by scanning).
pub fn frames_of_wellbore(store: &EpcStore, index: &PackageIndex, wellbore: &str) -> Vec<Box<dyn DynObject>> {
    index
        .dependents_of(wellbore)
        .into_iter()
        .filter(|pointer| pointer.qualified_type.ends_with("WellboreFrameRepresentation"))
        .filter_map(|pointer| store.get_object(&pointer.uri).ok())
        .collect()
}

/// What a section takes from a package when nobody chose: **every** well —
/// trajectories and frames, a frame carrying the strata and folding into its
/// trajectory's line (ADR-0015) — and every surface and grid a
/// section cuts ([`is_cut_type`]). By uri, in the package's order.
pub fn selection(store: &EpcStore) -> (Vec<String>, Vec<String>) {
    let resources = store.resources();
    let wells = resources
        .iter()
        .filter(|one| {
            one.qualified_type.ends_with("WellboreTrajectoryRepresentation")
                || one.qualified_type.ends_with("WellboreFrameRepresentation")
        })
        .map(|one| one.uri.clone())
        .collect();
    let surfaces = resources
        .iter()
        .filter(|one| is_cut_type(&one.qualified_type))
        .map(|one| one.uri.clone())
        .collect();
    (wells, surfaces)
}
