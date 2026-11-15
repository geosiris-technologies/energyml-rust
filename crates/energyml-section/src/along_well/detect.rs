//! What a frame offers a geosteering section: facts of the index only.
//!
//! These are facts, not the choice of a view: a face decides what to draw
//! from them. The name `offers` keeps clear of `shape_candidates` (the
//! index's own word) and of the inheritance module's `Candidate`.

use energyml_domain::package_index::{CandidateRole, PackageIndex};

use super::model::CandidateNeedAsk;

/// What a frame offers a geosteering section - facts of the index only: no
/// array is read (the signature takes no array source).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Offer {
    /// The frame (full uri).
    pub frame: String,
    /// The trajectory the frame references, when it does.
    pub trajectory: Option<String>,
    /// Properties with a matrix of two dimensions per node, and what a view must still be told.
    pub curtains: Vec<(String, Vec<CandidateNeedAsk>)>,
    /// Radius and angle pairs on the nodes (the DoD).
    pub boundaries: Vec<BoundaryOffer>,
}

/// A radius and the angles it pairs with.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BoundaryOffer {
    /// The radii (full uri).
    pub radius: String,
    /// The angles (full uri).
    pub angle: String,
}

/// What `frame` (a full uri) offers.
///
/// Only [`PackageIndex::shape_candidates`] decides what a property is: a
/// matrix of two dimensions is a curtain, a radius with its angles a boundary.
/// The support is filtered there: a grid gives nothing. The trajectory is the
/// target of the frame's reference whose path ends with `Trajectory`.
pub fn offers(index: &PackageIndex, frame: &str) -> Offer {
    let mut curtains = Vec::new();
    let mut boundaries = Vec::new();
    for candidate in index.shape_candidates(frame) {
        match &candidate.role {
            CandidateRole::Matrix { dims } if dims.len() == 2 => {
                let needs = candidate.needs.iter().cloned().map(CandidateNeedAsk::from).collect();
                curtains.push((candidate.uri.clone(), needs));
            }
            CandidateRole::Polar { angles: Some(angle) } => boundaries.push(BoundaryOffer {
                radius: candidate.uri.clone(),
                angle: angle.clone(),
            }),
            _ => {}
        }
    }
    let uuid = energyml_core::identity::parse_address(frame).0;
    let trajectory = index
        .references_of(&uuid)
        .into_iter()
        .find(|pointer| pointer.path.ends_with("Trajectory"))
        .map(|pointer| pointer.uri);
    Offer {
        frame: frame.to_string(),
        trajectory,
        curtains,
        boundaries,
    }
}
