//! Reading a well: its head, its path, its strata (ADR-0015).
//!
//! Contract: [`read_well`].
//!
//! # One reading of each thing
//!
//! Nothing is recomputed here; every piece is the library's own, the one the
//! 3D view and the profile read:
//!
//! - **the head** is `energyml_geometry::well::head_of` — the datum the
//!   trajectory's `MdInterval` names, **never the first node** (a trajectory
//!   whose first node is at MD 2.5 is collared 2.5 above it). A frame is
//!   collared where **its trajectory** is, as `heads::of_objects` says;
//! - **the nodes** are `scene::object_of(..).drawable_in(Projected)` — the very
//!   points the 3D draws, so a node cannot sit in one place there and in
//!   another here;
//! - **the MD of each node** is `strata_by_md::node_mds`, the function the strata
//!   painting reads. The two answers must agree on how many nodes there are:
//!   when they do not, the MD is withheld **and said** rather than given to
//!   nodes by guesswork — an MD on the wrong node is a well graduated wrongly;
//! - **the intervals** are `strata_by_md::Painting::intervals`, by the same
//!   rule as the colours of the nodes (*a node opens the interval*).
//!
//! Everything then goes through [`frame::to_scene`]: head and nodes in one
//! call, so they cannot be moved apart.
//!
//! # One wellbore, one key
//!
//! Lutetia draws a borehole as two trajectories and two frames. [`WellRead::wellbore`]
//! is what lets `assemble` fold them into one line: the uuid of the feature
//! the chain *representation → wellbore interpretation → wellbore feature*
//! ends on — the interpretation is the trajectory's, else the frame's own.
//! A representation of a WITSML `Wellbore` is keyed by that wellbore. With no
//! chain, a frame is keyed by the trajectory it hangs on, and a trajectory by
//! itself, so the frame and the trajectory it hangs on are still one line.
//!
//! # What this source cannot do, and the door that does
//!
//! [`ValueSource`] resolves a uuid; it **lists nothing**. Two things need a
//! listing: the package's `GraphicalInformationSet`s (the colour cascade of a
//! unit) and the frames of a wellbore (a frame or trajectory that states no
//! unit itself is painted from its sisters). [`read_well`] has neither, so a
//! unit is left without a colour (the section draws the well's own, never a
//! transparent one) and a silent object says why it has no interval.
//! [`read_well_with`] takes both in a [`WellContext`], for the face that holds
//! the package index.

use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::ArraySource as ValueSource;
use energyml_domain::color::Rgba;
use energyml_domain::strata_by_md::{self, Painting};
use energyml_domain::well_strata::{Known, Refusal};
use energyml_geometry::{Placement, scene, well};

use crate::model::{Interval, ObjectId, Refused, WellKind, WellRead};
use crate::read::Scene;
use crate::read::frame::{frame_of, to_scene};

const FRAME: &str = "WellboreFrameRepresentation";
const TRAJECTORY: &str = "WellboreTrajectoryRepresentation";

/// The package's context, under the name the wells' half first gave it: it now
/// serves the surfaces too ([`crate::read::PackageContext`]).
pub use crate::read::{FramesOf, PackageContext as WellContext};

/// One wellbore trajectory or frame, read in the scene's system: its head (the
/// datum, never the first node), its nodes and their MD, its strata intervals.
///
/// Without the package's colours or the frames of its wellbore — see the
/// module; [`read_well_with`] takes them.
// A refusal is the cold path, met once per object, and it carries the object's
// whole identity on purpose: boxing it would change every construction site
// for no gain anyone could measure.
#[allow(clippy::result_large_err)]
pub fn read_well(source: &dyn ValueSource, store: &str, uri: &str, scene: &Scene<'_>) -> Result<WellRead, Refused> {
    read_well_with(source, store, uri, scene, &WellContext::default())
}

/// [`read_well`], with what a listing of the package gives.
#[allow(clippy::result_large_err)]
pub fn read_well_with(
    source: &dyn ValueSource,
    store: &str,
    uri: &str,
    scene: &Scene<'_>,
    context: &WellContext<'_>,
) -> Result<WellRead, Refused> {
    let located = locate(source, store, uri)?;
    let Located { held, object, name, kind, .. } = &located;
    let (object, name, kind) = (object.clone(), name.as_str(), *kind);
    let refuse = |object: &ObjectId, reason: String| Refused { object: object.clone(), reason };
    let root = held.as_reflect();
    let trajectory = located.trajectory();
    let head = well::head_of(trajectory, source).map_err(|why| refuse(&object, format!("`{name}`: {why}")))?;

    // The nodes the 3D draws, placed in the projected system of the object.
    let drawn = scene::object_of(root, source, uri)
        .and_then(|shape| shape.drawable_in(Placement::Projected))
        .map_err(|why| refuse(&object, format!("`{name}`: {why}")))?;
    let dropped = drawn.dropped;
    let mut points: Vec<[f64; 3]> = Vec::with_capacity(drawn.points.len() + 1);
    points.push(head);
    points.extend(drawn.points);

    // Into the scene's system and, for an object in time, onto its depth axis.
    let frame = frame_of(source, root).map_err(|why| refuse(&object, format!("`{name}`: {why}")))?;
    to_scene(&mut points, &frame, scene).map_err(|why| refuse(&object, format!("`{name}`: {why}")))?;
    let path = points.split_off(1);
    let head = points[0];

    // The MD of each node, when it can be given to them.
    let mut no_md: Option<String> = None;
    let md = match strata_by_md::node_mds(root, source) {
        Ok(mds) if mds.len() == path.len() => Some(mds),
        Ok(mds) => {
            no_md = Some(format!(
                "`{name}` is placed at {} node(s) and states {} measured depth(s) ({dropped} node(s) were dropped \
                 for lacking a location), so no MD can be given to a position without a guess",
                path.len(),
                mds.len()
            ));
            None
        }
        Err(why) => {
            no_md = Some(why.to_string());
            None
        }
    };

    let (intervals, strata, borrowed) = strata_of(root, name, source, md.as_deref(), no_md.as_deref(), context);
    let wellbore = wellbore_of(root, trajectory, source);

    Ok(WellRead { object, kind, wellbore, head, path, md, intervals, strata, borrowed, head_only: false })
}

/// A well's object, found: what its identity says and the trajectory it is
/// collared on — the part of [`read_well_with`] that reads no array.
struct Located {
    held: Box<dyn DynObject>,
    object: ObjectId,
    name: String,
    kind: WellKind,
    /// The trajectory a frame hangs on; `None` for a trajectory (it is `held`).
    frame_trajectory: Option<Box<dyn DynObject>>,
}

impl Located {
    fn trajectory(&self) -> &dyn Reflect {
        match &self.frame_trajectory {
            Some(trajectory) => trajectory.as_reflect(),
            None => self.held.as_reflect(),
        }
    }
}

#[allow(clippy::result_large_err)]
fn locate(source: &dyn ValueSource, store: &str, uri: &str) -> Result<Located, Refused> {
    let mut object = ObjectId {
        store: store.to_string(),
        uri: uri.to_string(),
        uuid: String::new(),
        title: String::new(),
        qualified_type: String::new(),
    };
    let refuse = |object: &ObjectId, reason: String| Refused { object: object.clone(), reason };

    let held = source
        // By the uri the section was given: two versions of an object share a uuid.
        .read_object_at(uri)
        .map_err(|why| refuse(&object, format!("`{uri}` could not be read: {why}")))?;
    let root = held.as_reflect();
    let info = root.type_info();
    object.uuid = identity::uuid_of_reflect(root).unwrap_or_default().to_ascii_lowercase();
    object.title = identity::title_of_reflect(root).unwrap_or_default().to_string();
    object.qualified_type = energyml_core::json::qualified_type_of(info);
    let name = if object.title.is_empty() { uri.to_string() } else { object.title.clone() };

    let kind = match info.type_key {
        FRAME => WellKind::Frame,
        TRAJECTORY => WellKind::Trajectory,
        other => {
            return Err(refuse(
                &object,
                format!(
                    "`{name}` is a `{other}`: only a wellbore trajectory or a wellbore frame is a well of a section"
                ),
            ));
        }
    };

    // A frame is collared where its trajectory is.
    let frame_trajectory = match kind {
        WellKind::Trajectory => None,
        WellKind::Frame => Some(
            trajectory_of_frame(root, source)
                .map_err(|why| refuse(&object, format!("`{name}` is a wellbore frame {why}")))?,
        ),
    };
    Ok(Located { held, object, name, kind, frame_trajectory })
}

/// What a well is, and where it is collared, **without reading any array**: the
/// object, its kind, its wellbore, and its head in the scene's system — what
/// the corridor filter decides on (`read::contribution_cached`).
#[derive(Clone, Debug, PartialEq)]
pub struct HeadRead {
    /// The object.
    pub object: ObjectId,
    /// Frame or trajectory.
    pub kind: WellKind,
    /// The wellbore it describes, as [`WellRead::wellbore`].
    pub wellbore: Option<String>,
    /// The head in the scene's system, `[x, y, z]` — the very point
    /// [`read_well_with`] gives `WellRead::head`.
    pub head: [f64; 3],
}

impl HeadRead {
    /// The well as `assemble` needs it **when no section is near** (`head_only`): its
    /// identity, its wellbore and its head, no node and no interval. Never
    /// drawn — it is only ever the reason a well is refused as too far, which
    /// says nothing but the head.
    pub fn stub(&self) -> WellRead {
        WellRead {
            object: self.object.clone(),
            kind: self.kind,
            wellbore: self.wellbore.clone(),
            head: self.head,
            path: Vec::new(),
            md: None,
            intervals: Vec::new(),
            strata: None,
            borrowed: false,
            head_only: true,
        }
    }
}

/// The [`HeadRead`] of a well. Any refusal here is **not** the well's verdict:
/// the caller reads the well in full, whose refusal is the one the catalogue
/// says (the order of the checks differs here).
#[allow(clippy::result_large_err)]
pub fn read_head(source: &dyn ValueSource, store: &str, uri: &str, scene: &Scene<'_>) -> Result<HeadRead, Refused> {
    let located = locate(source, store, uri)?;
    let refuse = |reason: String| Refused { object: located.object.clone(), reason: format!("`{}`: {reason}", located.name) };
    let root = located.held.as_reflect();
    let trajectory = located.trajectory();
    let head = well::head_of(trajectory, source).map_err(|why| refuse(why.to_string()))?;
    let frame = frame_of(source, root).map_err(refuse)?;
    let mut points = [head];
    to_scene(&mut points, &frame, scene).map_err(refuse)?;
    let wellbore = wellbore_of(root, trajectory, source);
    Ok(HeadRead { object: located.object, kind: located.kind, wellbore, head: points[0] })
}

/// The trajectory a frame hangs on, or what is wrong with it — a sentence that
/// continues *"is a wellbore frame …"*.
fn trajectory_of_frame(frame: &dyn Reflect, source: &dyn ValueSource) -> Result<Box<dyn DynObject>, String> {
    let Some(FieldValue::Object(reference)) = frame.field("Trajectory") else {
        return Err("that names no trajectory".to_string());
    };
    let uri = identity::uri_of_reflect(reference).ok_or_else(|| "whose trajectory reference carries no uuid".to_string())?;
    // Followed by the reference: the version it names is the trajectory read.
    source.object_of(reference).ok_or_else(|| format!("whose trajectory {uri} is not in this package"))
}

/// The intervals of a well, or why it has none.
fn strata_of(
    root: &dyn Reflect,
    name: &str,
    source: &dyn ValueSource,
    md: Option<&[f64]>,
    no_md: Option<&str>,
    context: &WellContext<'_>,
) -> (Vec<Interval>, Option<String>, bool) {
    let frames_of = |wellbore: &str| context.frames_of.map(|asked| asked(wellbore)).unwrap_or_default();
    let known = Known { graphics: context.graphics, units: &[] };
    let painting = match strata_by_md::of_representation(root, &frames_of, None, source, &known) {
        Ok(painting) => painting,
        Err(refusal) => {
            let mut why = refusal.to_string();
            // An absence found without having been able to look is not an
            // absence: say what could not be consulted.
            if matches!(refusal, Refusal::Absent { .. })
                && context.frames_of.is_none()
                && !energyml_domain::well_strata::states_units(root)
            {
                why.push_str(
                    " (the other frames of its wellbore were not consulted: this source resolves a uuid and cannot \
                     list a package)",
                );
            }
            return (Vec::new(), Some(why), false);
        }
    };
    let borrowed = matches!(painting, Painting::Carried(_));
    // A carried painting is looked up at the MD of each node: without them
    // there is nothing to say.
    if matches!(painting, Painting::Carried(_)) && md.is_none() {
        let why = no_md.unwrap_or("its nodes have no measured depth");
        return (Vec::new(), Some(format!("`{name}` is painted from its wellbore's frames by MD, and {why}")), borrowed);
    }
    let intervals = match painting.intervals(md.unwrap_or(&[]), &[]) {
        Ok(intervals) => intervals,
        Err(why) => return (Vec::new(), Some(why), borrowed),
    };
    if intervals.is_empty() {
        let notes = match &painting {
            Painting::Own(strata) => strata.notes.clone(),
            Painting::Carried(carried) => carried.notes.clone(),
        };
        let why = if notes.is_empty() {
            format!("`{name}`: no stratigraphic unit holds any of its intervals")
        } else {
            format!("`{name}`: no stratigraphic unit holds any of its intervals — {}", notes.join("; "))
        };
        return (Vec::new(), Some(why), borrowed);
    }
    // The interval carries the uri of its unit — what the front end later asks the
    // colour walks for, so that an edited colour shows without a new catalogue.
    // It used to be **found** by title and colour among the painting's units,
    // and refused when two units shared both.
    let intervals = intervals
        .into_iter()
        .map(|one| Interval {
            from: one.from,
            to: one.to,
            colour: one.colour.map(hex),
            unit: one.unit,
            unit_uri: one.unit_uri,
        })
        .collect();
    (intervals, None, borrowed)
}

/// `#rrggbb`, or `#rrggbbaa` when the colour is not opaque.
fn hex(colour: Rgba) -> String {
    let [r, g, b, a] = colour.to_bytes();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// What makes a frame and its trajectory one wellbore.
///
/// The wellbore feature reached by *interpretation → feature* — the
/// interpretation of the trajectory, else the representation's own — or the
/// WITSML wellbore a representation names; failing that the trajectory's own
/// uuid, which is also what a frame hanging on it gets.
///
/// **The key is a uuid on purpose**: it names a *wellbore* — a concept two
/// objects share (a frame and its trajectory), a feature that has no version
/// in practice — and not one object of the package. What is *read* on the way
/// is followed by the reference, so the version it names is the version read.
fn wellbore_of(root: &dyn Reflect, trajectory: &dyn Reflect, source: &dyn ValueSource) -> Option<String> {
    const REPRESENTED: [&str; 2] = ["RepresentedObject", "RepresentedInterpretation"];
    fn named(object: &dyn Reflect) -> Option<&dyn Reflect> {
        REPRESENTED.iter().find_map(|field| match object.field(field)? {
            FieldValue::Object(reference) if identity::uuid_of_reflect(reference).is_some_and(|uuid| !uuid.is_empty()) => {
                Some(reference)
            }
            _ => None,
        })
    }
    let chain = [named(trajectory), named(root)].into_iter().flatten().find_map(|reference| {
        let represented = identity::uuid_of_reflect(reference)?.to_ascii_lowercase();
        let object = source.object_of(reference)?;
        let reflect = object.as_reflect();
        if reflect.type_info().package == "witsml" {
            // A representation of a WITSML wellbore: that is the wellbore.
            return Some(represented);
        }
        // A wellbore interpretation: its feature, when it is one.
        let Some(FieldValue::Object(reference)) = reflect.field("InterpretedFeature") else {
            return None;
        };
        let feature = identity::uuid_of_reflect(reference).filter(|uuid| !uuid.is_empty())?.to_ascii_lowercase();
        match source.object_of(reference) {
            Some(found) if found.as_reflect().type_info().type_key != "WellboreFeature" => None,
            _ => Some(feature),
        }
    });
    chain.or_else(|| {
        identity::uuid_of_reflect(trajectory).filter(|uuid| !uuid.is_empty()).map(str::to_ascii_lowercase)
    })
}
