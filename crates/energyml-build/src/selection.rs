//! Selecting objects by rings - a step **before** the operations, not a change to them.
//!
//! The use case: when selecting objects for operations, apply ring filters
//! (outer/inner) without modifying the operations. The user selects all the
//! boreholes at once by ticking everything, then gives one or several
//! inner/outer rings to deselect a subset. The command line also needs a kind
//! of 'selection' with rings (inner and outer) and parameters (inversion,
//! prefilter with uuids, objects entirely inside or merely touched, etc.) that
//! returns a list of uuids/uris, so that operations can be chained
//! automatically.
//!
//! A selection answers *which of these objects*, as lists of uris; an operation
//! then takes them as it takes any other. [`clip`](crate::ops) is the opposite
//! question - it cuts a shape - and the two do not share a decision beyond the
//! point-in-ring test, which is `energyml_geometry::clip::inside_ring` (even-odd,
//! the border counts as inside).
//!
//! # The region
//!
//! A point is **in the region** when it is inside at least one `outer` ring (no
//! `outer` at all means everywhere) and inside no `inner` ring. A node exactly on
//! a ring's border is inside that ring - so on an `inner` it is *out* of the
//! region.
//!
//! # The three tests
//!
//! - [`Test::Inside`] - every node, in plan, is in the region. Judged on nodes:
//!   an edge that leaves the region between two nodes inside does not change it
//!   (a triangle whose corners are all in does not look at its sides);
//! - [`Test::Touches`] - some node is in the region, **or** some edge (a segment
//!   of a line, a side of a triangle) passes through it without a node inside.
//!   An edge is cut at every point it meets a ring border and the pieces are
//!   tested at their midpoints, so a hole in the middle of an edge is seen and a
//!   ring crossing between two outside nodes is too;
//! - [`Test::Head`] - the head of a well is in the region. The head is the
//!   **first node** of a wellbore trajectory or frame, placed. Any other object
//!   has no head: it is judged as `Touches`, and a note says so.
//!
//! [`Request::invert`] swaps kept and dropped. It never touches `refused`: an
//! object that could not be judged is neither.
//!
//! # What is refused, by name
//!
//! An object that is not in the package, cannot be read, has no node, names no
//! horizontal system when the rings name one, or whose system cannot be brought
//! into the rings' - each goes to [`Selected::refused`] with its reason. Nothing
//! is put in `kept` or `dropped` by default.
//!
//! The rings' coordinates are in the units of their system as the reprojection
//! backend speaks them: metres for a projected system, degrees (longitude,
//! latitude) for a geographic one.

use energyml_domain::reproject::{self, System};
use energyml_geometry::Placement;
use energyml_geometry::clip::inside_ring;
use energyml_geometry::scene;
use energyml_storage::EpcStore;
use serde::{Deserialize, Serialize};

/// The rings of a selection: the region is inside one `outer` and inside no `inner`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rings {
    /// The EPSG code the rings' coordinates are in. `None`: the objects are
    /// compared as they stand, in their own coordinates, and a note says so.
    #[serde(default)]
    pub epsg: Option<u32>,
    /// The rings a node must be inside one of. None: everywhere.
    #[serde(default)]
    pub outer: Vec<Vec<[f64; 2]>>,
    /// The rings a node must be inside none of.
    #[serde(default)]
    pub inner: Vec<Vec<[f64; 2]>>,
}

impl Rings {
    /// A ring of fewer than three vertices, or with a coordinate that is not a
    /// number, encloses nothing - refused rather than judged as an empty region.
    pub fn check(&self) -> Result<(), String> {
        for (role, rings) in [("outer", &self.outer), ("inner", &self.inner)] {
            for (at, ring) in rings.iter().enumerate() {
                if ring.len() < 3 {
                    return Err(format!(
                        "the {role} ring #{at} has {} vertices: a ring needs at least three",
                        ring.len()
                    ));
                }
                if ring.iter().any(|[x, y]| !x.is_finite() || !y.is_finite()) {
                    return Err(format!(
                        "the {role} ring #{at} has a coordinate that is not a finite number"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Whether a point is in the region.
    pub fn contains(&self, point: [f64; 2]) -> bool {
        (self.outer.is_empty() || self.outer.iter().any(|ring| inside_ring(point, ring)))
            && !self.inner.iter().any(|ring| inside_ring(point, ring))
    }

    fn borders(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        self.outer.iter().chain(self.inner.iter()).flat_map(|ring| {
            let n = ring.len();
            (0..n).map(move |at| (ring[at], ring[(at + 1) % n]))
        })
    }
}

/// How an object is judged against the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Test {
    /// Every node is in the region.
    Inside,
    /// A node is in the region, or an edge passes through it.
    Touches,
    /// The head of a well is in the region.
    Head,
}

impl Test {
    /// A test by the name the command line and the front end type.
    pub fn parse(name: &str) -> Result<Test, String> {
        match name {
            "inside" => Ok(Test::Inside),
            "touches" => Ok(Test::Touches),
            "head" => Ok(Test::Head),
            other => Err(format!(
                "unknown test `{other}`: one of `inside`, `touches` or `head`"
            )),
        }
    }
}

/// What the front end and the command line ask: the wire's contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The objects to judge, by uri or uuid.
    pub uris: Vec<String>,
    /// The region.
    pub rings: Rings,
    /// How to judge.
    pub test: Test,
    /// Swap kept and dropped.
    #[serde(default)]
    pub invert: bool,
}

/// An object that could not be judged, and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    /// The object, as it was asked.
    pub uri: String,
    /// The reason, in a sentence.
    pub why: String,
}

/// What a selection answers: three lists that never overlap, and what the
/// caller should know of how they were made.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selected {
    /// The objects selected, in the order asked.
    pub kept: Vec<String>,
    /// The objects judged and left out.
    pub dropped: Vec<String>,
    /// The objects that could not be judged.
    pub refused: Vec<Refused>,
    /// What was assumed or substituted on the way.
    pub notes: Vec<String>,
}

/// One object in plan, as the judgement sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footprint {
    /// Every node, `(x, y)`.
    pub nodes: Vec<[f64; 2]>,
    /// Pairs of ranks into `nodes`: the segments of a line, the sides of a triangle.
    pub edges: Vec<[u32; 2]>,
    /// The head of a well, when the object is one.
    pub head: Option<[f64; 2]>,
}

/// The verdict on one object, before any inversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    /// Whether the test holds.
    pub holds: bool,
    /// Set when the test asked for was not the one applied (`Head` on an object
    /// that is not a well).
    pub substituted: bool,
}

/// Judge one footprint against the region - pure, no package.
///
/// # Errors
///
/// A footprint with no node: nothing can be said of it, and `Inside` over an
/// empty set would be true of everything.
pub fn judge(footprint: &Footprint, rings: &Rings, test: Test) -> Result<Judgement, String> {
    if footprint.nodes.is_empty() {
        return Err("it has no node".to_string());
    }
    let mut substituted = false;
    let test = match (test, footprint.head) {
        (Test::Head, Some(head)) => {
            return Ok(Judgement {
                holds: rings.contains(head),
                substituted,
            });
        }
        (Test::Head, None) => {
            substituted = true;
            Test::Touches
        }
        (other, _) => other,
    };
    let holds = match test {
        Test::Inside => footprint.nodes.iter().all(|&node| rings.contains(node)),
        _ => {
            footprint.nodes.iter().any(|&node| rings.contains(node))
                || footprint.edges.iter().any(|&[a, b]| {
                    match (footprint.nodes.get(a as usize), footprint.nodes.get(b as usize)) {
                        (Some(&a), Some(&b)) => edge_touches(a, b, rings),
                        _ => false,
                    }
                })
        }
    };
    Ok(Judgement { holds, substituted })
}

/// Whether a segment passes through the region: cut at every point it meets a
/// ring border, then each piece's midpoint (and the ends) tested. The region
/// cannot change inside a piece, so this is exact.
fn edge_touches(a: [f64; 2], b: [f64; 2], rings: &Rings) -> bool {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (lo, hi) = ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]);
    let mut cuts = vec![0.0, 1.0];
    for (p, q) in rings.borders() {
        // The border's box against the edge's: most are far away.
        if p[0].max(q[0]) < lo[0]
            || p[0].min(q[0]) > hi[0]
            || p[1].max(q[1]) < lo[1]
            || p[1].min(q[1]) > hi[1]
        {
            continue;
        }
        let e = [q[0] - p[0], q[1] - p[1]];
        let denom = d[0] * e[1] - d[1] * e[0];
        if denom.abs() < 1e-18 {
            continue; // parallel: the neighbours of this border cut where it begins and ends
        }
        let w = [p[0] - a[0], p[1] - a[1]];
        let t = (w[0] * e[1] - w[1] * e[0]) / denom;
        let u = (w[0] * d[1] - w[1] * d[0]) / denom;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            cuts.push(t);
        }
    }
    cuts.sort_by(|x, y| x.total_cmp(y));
    cuts.windows(2).any(|pair| {
        let mid = (pair[0] + pair[1]) / 2.0;
        rings.contains([a[0] + d[0] * mid, a[1] + d[1] * mid])
    })
}

/// The uris of the package's objects a selection can be asked of: those whose
/// type this build reads as a geometry, narrowed to the types given (substring,
/// as `list --type`; none: all).
pub fn candidates(store: &EpcStore, types: &[String]) -> Vec<String> {
    store
        .resources()
        .into_iter()
        .filter(|one| {
            let concept = one
                .qualified_type
                .rsplit('.')
                .next()
                .unwrap_or(&one.qualified_type);
            energyml_geometry::geometry::supports(concept.strip_prefix("obj_").unwrap_or(concept))
        })
        .filter(|one| {
            types.is_empty()
                || types
                    .iter()
                    .any(|t| energyml_storage::resource::type_matches(&one.qualified_type, t))
        })
        .map(|one| one.uri)
        .collect()
}

/// A wellbore trajectory or frame: the objects that have a head.
fn is_well(qualified_type: &str) -> bool {
    qualified_type.ends_with("WellboreTrajectoryRepresentation")
        || qualified_type.ends_with("WellboreFrameRepresentation")
}

/// Read one object placed, in the rings' system when it has one.
fn footprint_of(
    store: &EpcStore,
    asked: &str,
    rings_system: Option<&System>,
    backend: &dyn reproject::Reprojector,
) -> Result<Footprint, String> {
    // **By the store's one resolution** (`resolve`: a uri, a short uri or a bare
    // uuid, each by its own rule): an uuid held in two versions is refused with
    // its candidates, never read as one of them.
    let summary = match store.resolve(asked) {
        Ok(summary) => summary.clone(),
        Err(energyml_storage::Error::NotFound(_)) => {
            return Err("it is not an object of the package".to_string());
        }
        Err(other) => return Err(other.to_string()),
    };
    let object = store
        .get_object(&summary.uri)
        .map_err(|why| format!("it does not read: {why}"))?;
    let built = scene::object_of(object.as_reflect(), store, &summary.uri)
        .map_err(|why| format!("no geometry could be read: {why}"))?;
    let drawable = built
        .drawable_in(Placement::Projected)
        .map_err(|why| format!("it cannot be placed: {why}"))?;
    let mut points = drawable.points;
    if let Some(target) = rings_system {
        let own = match &built.crs {
            None => {
                return Err(format!(
                    "it names no coordinate system and the rings are in EPSG:{}",
                    target.epsg
                ));
            }
            Some(crs) => System::of(crs)
                .map_err(|why| format!("its system cannot be compared with the rings': {why}"))?,
        };
        points = reproject::reproject_placed(&points, &own, target, backend).map_err(|why| {
            format!(
                "reprojecting from EPSG:{} to EPSG:{}: {why}",
                own.epsg, target.epsg
            )
        })?;
    }
    let nodes: Vec<[f64; 2]> = points.iter().map(|p| [p[0], p[1]]).collect();
    let mut edges = drawable.lines;
    for [a, b, c] in drawable.triangles {
        edges.extend([[a, b], [b, c], [c, a]]);
    }
    let head = if is_well(&summary.qualified_type) {
        nodes.first().copied()
    } else {
        None
    };
    Ok(Footprint { nodes, edges, head })
}

/// Judge these objects of a package against the rings.
pub fn select(store: &EpcStore, uris: &[String], request: &Request) -> Selected {
    let mut out = Selected::default();
    let mut seen = std::collections::HashSet::new();
    let uris: Vec<&String> = uris
        .iter()
        .filter(|uri| seen.insert(uri.to_ascii_lowercase()))
        .collect();
    if let Err(why) = request.rings.check() {
        out.refused = uris
            .iter()
            .map(|uri| Refused {
                uri: (*uri).clone(),
                why: format!("the rings are refused: {why}"),
            })
            .collect();
        return out;
    }
    let target = request.rings.epsg.map(|epsg| System {
        epsg: u64::from(epsg),
        unit: "m".to_string(),
    });
    if target.is_none() {
        out.notes.push(
            "the rings name no EPSG code: every object is compared in its own coordinates, without reprojection".to_string(),
        );
    }
    let backend = energyml_proj::Proj4::new();
    let mut substituted = Vec::new();
    for uri in uris {
        let verdict = footprint_of(store, uri, target.as_ref(), &backend)
            .and_then(|footprint| judge(&footprint, &request.rings, request.test));
        match verdict {
            Err(why) => out.refused.push(Refused {
                uri: uri.clone(),
                why,
            }),
            Ok(judged) => {
                if judged.substituted {
                    substituted.push(uri.clone());
                }
                if judged.holds != request.invert {
                    out.kept.push(uri.clone());
                } else {
                    out.dropped.push(uri.clone());
                }
            }
        }
    }
    if !substituted.is_empty() {
        let shown: Vec<&str> = substituted.iter().take(5).map(String::as_str).collect();
        out.notes.push(format!(
            "`head` was asked of {} object(s) that are not wellbore trajectories or frames, and has no meaning for them: they were judged as `touches` ({}{})",
            substituted.len(),
            shown.join(", "),
            if substituted.len() > shown.len() { ", …" } else { "" },
        ));
    }
    out
}
