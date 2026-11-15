//! What the index says of a property's neighbours: its siblings, and, along a
//! well, what the shape of each property makes it.
//!
//! Both answer from the **sheets** alone: no object is read, no array is
//! read. The comparison of two properties is not written here, it is
//! [`property::kinship::kinship`](crate::property::kinship::kinship), the one
//! rule that a check without an index calls too.

use crate::package_index::{IndexNode, PackageIndex};
use crate::property::kinship::{KinFacts, Kinship, Sibling, kinship};
use crate::property::IndexableElement;
use crate::property::link::qualifier_link;
use crate::units::Dimension;

/// The representations a property can be laid along a well on (the frame of a
/// well log, its seismic variant, a blocked well).
///
/// `TODO(verify)`: the exact list of the desktop's `frame_log` supports; this
/// one is the list of the design of the log views.
const ALONG_WELL_SUPPORTS: &[&str] = &[
    "WellboreFrameRepresentation",
    "SeismicWellboreFrameRepresentation",
    "BlockedWellboreRepresentation",
];

/// What the shape of a property along a well makes it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CandidateRole {
    /// `[m, n, ...]`, every dimension >= 2: an image, a curtain, a cube.
    Matrix {
        /// The dimensions per element, slowest first.
        dims: Vec<u64>,
    },
    /// `[k]`, `k >= 3`, a length: radii, paired with angles when exactly one
    /// sibling holds them.
    Polar {
        /// Uri of the angles, when exactly one sibling holds them.
        angles: Option<String>,
    },
    /// `[k]`, `k >= 3`, neither a length nor an angle: one value per sector.
    Sectors {
        /// The number of sectors.
        count: u64,
    },
    /// One value per element.
    Scalar,
}

/// What a view must be told before it can draw a candidate.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CandidateNeed {
    /// No axis source in the file (no labels, no convention-B facet): axes to declare.
    Axes,
    /// Angle origin and sense: never in the file.
    AngleConvention,
    /// Several angle siblings fit: the caller chooses.
    ChooseAngles {
        /// Their uris, in the order of the inventory.
        candidates: Vec<String>,
    },
}

/// One property of a support along a well, with what it makes of it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShapeCandidate {
    /// Full uri of the property.
    pub uri: String,
    /// What its shape makes it.
    pub role: CandidateRole,
    /// What a view must be told to draw it.
    pub needs: Vec<CandidateNeed>,
}

impl PackageIndex {
    /// The siblings of a property (a full uri, or a bare uuid at the entry), in
    /// the inventory order of its representation's properties. Reads no
    /// object.
    ///
    /// The kinds are compared as written (the index has no resolver), see
    /// [`kinship`]. An unknown property, or one with no supporting
    /// representation, has none.
    pub fn siblings(&self, property: &str) -> Vec<Sibling> {
        let (Some(this_uri), Some(sheet)) = (self.uri_of(property), self.sheet(property)) else {
            return Vec::new();
        };
        let (Some(this), Some(support)) = (
            KinFacts::of_sheet(this_uri, sheet),
            sheet.supporting_representation.as_deref(),
        ) else {
            return Vec::new();
        };
        self.properties_of(support)
            .filter(|(uri, _)| !uri.eq_ignore_ascii_case(this_uri))
            .filter_map(|(uri, other)| {
                let other = KinFacts::of_sheet(uri, other)?;
                kinship(&this, &other).map(|kinship| Sibling {
                    uri: uri.to_string(),
                    kinship,
                })
            })
            .collect()
    }

    /// Facts on the properties of a support along a well: which are a matrix,
    /// a polar vector, sectors or a scalar, and what is missing to draw them.
    /// Reads no object and no array.
    ///
    /// `support` is a frame, a seismic frame or a blocked well, else nothing
    /// (a `[81, 81]` property on a grid is not a log). A property is skipped
    /// when it has a lookup table, a time dimension or realization indices
    /// (not facts of shape), when its shape is refused, and when it is the
    /// angle of a polar pair (it is the axis of the pair, not a layer).
    /// Same order as `properties_of`.
    pub fn shape_candidates(&self, support: &str) -> Vec<ShapeCandidate> {
        let Some(node) = self.node(support) else {
            return Vec::new();
        };
        if !is_along_well(node) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (uri, sheet) in self.properties_of(&node.uuid) {
            if !is_on_nodes_or_intervals(sheet.indexable)
                || sheet.lookup.is_some()
                || sheet.time_is_a_dimension()
                || sheet.has_realization_indices
            {
                continue;
            }
            let Ok(shape) = sheet.element_shape() else {
                continue;
            };
            let dims: Vec<u64> = shape.dims.iter().map(|&dim| dim as u64).collect();
            let candidate = if shape.is_scalar() {
                ShapeCandidate {
                    uri: uri.to_string(),
                    role: CandidateRole::Scalar,
                    needs: Vec::new(),
                }
            } else if shape.rank() >= 2 {
                if dims.iter().any(|&dim| dim < 2) {
                    continue;
                }
                let mut needs = Vec::new();
                // A refused link keeps `Axes`: the reading of the axes says why.
                if !matches!(qualifier_link(&sheet.facets), Ok(Some(_))) {
                    needs.push(CandidateNeed::Axes);
                }
                ShapeCandidate {
                    uri: uri.to_string(),
                    role: CandidateRole::Matrix { dims },
                    needs,
                }
            } else if dims[0] >= 3 {
                match sheet.uom.as_deref().and_then(Dimension::of) {
                    Some(Dimension::PlaneAngle) => continue,
                    Some(Dimension::Length) => {
                        let angles: Vec<String> = self
                            .siblings(uri)
                            .into_iter()
                            .filter_map(|sibling| match sibling.kinship {
                                Kinship::PolarPair { radius, angles } if radius.eq_ignore_ascii_case(uri) => {
                                    Some(angles)
                                }
                                _ => None,
                            })
                            .collect();
                        let mut needs = vec![CandidateNeed::AngleConvention];
                        let chosen = if angles.len() == 1 { angles.first().cloned() } else { None };
                        if angles.len() > 1 {
                            needs.push(CandidateNeed::ChooseAngles { candidates: angles });
                        }
                        ShapeCandidate {
                            uri: uri.to_string(),
                            role: CandidateRole::Polar { angles: chosen },
                            needs,
                        }
                    }
                    _ => ShapeCandidate {
                        uri: uri.to_string(),
                        role: CandidateRole::Sectors { count: dims[0] },
                        needs: Vec::new(),
                    },
                }
            } else {
                continue;
            };
            out.push(candidate);
        }
        out
    }
}

fn is_along_well(node: &IndexNode) -> bool {
    ALONG_WELL_SUPPORTS.contains(&&*node.type_key)
}

/// The elements a value along a well hangs on: the nodes of a frame, or the
/// intervals between them (RESQML TRG v2.2 §7.1: an interval is a node-to-node
/// span; the frame's `NodeCount - 1` intervals).
fn is_on_nodes_or_intervals(element: IndexableElement) -> bool {
    matches!(
        element,
        IndexableElement::Nodes
            | IndexableElement::Intervals
            | IndexableElement::Interval
            | IndexableElement::IntervalEdges
            | IndexableElement::IntervalsFromDatum
    )
}
