//! **The wellheads of many wells, in the coordinates a scene draws them in** —
//! what a profile projects (the wiring that was still to be done for the
//! well profile view).
//!
//! `well::head_of` says where a trajectory is collared: the datum its
//! `MdInterval` names, **projected** `x`/`y`, in the datum's own system. A
//! viewer draws in a *placement* (`local`, `projected` or `metres`), and the
//! track a profile follows was pointed on that drawing. So the head is brought
//! into the placement asked for before anything is measured:
//!
//! | placement | the head |
//! |---|---|
//! | `Projected` | `head_of` as it stands: the datum already *is* in the projected system |
//! | `Metres` | the same, the horizontal unit of the CRS taken to metres (a unit this build cannot convert **refuses**, never passes as metres) |
//! | `Local` | a trajectory with a `Geometry` draws in its CRS's own axes, so the head goes back through `Crs::from_projected`; one without has only the datum and is anchored as it stands |
//!
//! Nothing here reads a trajectory's array: a package of 2 700 wells is
//! 2 700 datum lookups, not 2 700 HDF5 reads.
//!
//! # One frame of reference, or a refusal
//!
//! A profile measures distances in a plane; two heads in two systems would be
//! measured against each other in one set of numbers that mean two things — a
//! wrong answer that looks like a right one. [`Heads::system`] is the system
//! every head shares, and [`of_objects`] **refuses the lot** when they do not,
//! naming the first two that disagree. Whether the *track* is in that system
//! is the caller's to check — it holds the track — and [`Heads::system`] is
//! what it compares with.
//!
//! # What is refused, and what is not
//!
//! One object that cannot be read goes to [`Heads::refused`] with its name and
//! its reason, and the others go on. A frame takes the head of **its
//! trajectory**; anything else — a surface, a WITSML trajectory — is refused as
//! what it is.

use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::{Crs, crs, units};
use energyml_storage::resource::ObjectSummary;

use crate::error::{Error, Result};
use crate::scene::Placement;
use crate::source::ValueSource;
use crate::well;

const FRAME: &str = "WellboreFrameRepresentation";
const TRAJECTORY: &str = "WellboreTrajectoryRepresentation";

/// The head of one well, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct WellHead {
    /// The object, as asked (the frame's, for a frame).
    pub id: String,
    /// Its citation title, or an empty string.
    pub title: String,
    /// The head in plan, in the placement asked for.
    pub xy: [f64; 2],
}

/// An object whose head could not be read, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadRefused {
    /// The object, as asked.
    pub id: String,
    /// The reason, in a sentence.
    pub why: String,
}

/// What [`of_objects`] returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heads {
    /// The heads read, in the order asked.
    pub heads: Vec<WellHead>,
    /// The objects that gave none.
    pub refused: Vec<HeadRefused>,
    /// The EPSG code of the system every head sits in, when its CRS states one.
    pub system: Option<u64>,
}

/// The way two heads can be told apart: the EPSG code and, outside `Metres`,
/// the horizontal unit — two systems that spell the same number differently
/// are two systems.
#[derive(Clone, Debug, PartialEq)]
struct System {
    epsg: Option<u64>,
    uom: Option<String>,
}

impl System {
    fn describe(&self) -> String {
        let epsg = self
            .epsg
            .map(|code| format!("EPSG:{code}"))
            .unwrap_or_else(|| "no EPSG code".to_string());
        match &self.uom {
            Some(uom) => format!("{epsg}, in {uom}"),
            None => epsg,
        }
    }
}

/// The trajectories of a package, by uri — what "every well" means here: one
/// head per hole. A frame hangs on a trajectory and would repeat its head, so
/// the frames are left out unless they are asked for by name.
pub fn trajectories_of(summaries: &[ObjectSummary]) -> Vec<String> {
    summaries
        .iter()
        .filter(|one| one.qualified_type.ends_with(TRAJECTORY))
        .map(|one| one.uri.clone())
        .collect()
}

/// The heads of these objects — wellbore trajectories or frames, resolved by
/// `source` from what `asked` says (a uri or a uuid, as the source takes it) —
/// placed as asked. See the module for the refusals.
///
/// # Errors
///
/// Only when the heads read do not share one system; everything else is a
/// [`HeadRefused`].
pub fn of_objects(source: &dyn ValueSource, asked: &[String], placement: Placement) -> Result<Heads> {
    let mut out = Heads::default();
    let mut seen: Option<(System, String)> = None;
    for id in asked {
        match head_of_one(source, id, placement) {
            Ok((head, system)) => {
                match &seen {
                    None => seen = Some((system, id.clone())),
                    Some((first, first_id)) if *first != system => {
                        return Err(Error::malformed(
                            id,
                            format!(
                                "this well's head is in {} but `{first_id}`'s is in {}: a profile measures in one \
                                 plane, and two systems would be measured against each other as if they were one",
                                system.describe(),
                                first.describe()
                            ),
                        ));
                    }
                    Some(_) => {}
                }
                out.heads.push(head);
            }
            Err(why) => out.refused.push(HeadRefused { id: id.clone(), why }),
        }
    }
    out.system = seen.and_then(|(system, _)| system.epsg);
    Ok(out)
}

/// One object: its head, and the system it is in.
fn head_of_one(
    source: &dyn ValueSource,
    id: &str,
    placement: Placement,
) -> std::result::Result<(WellHead, System), String> {
    // `id` is a uri or a bare uuid, as the caller holds it: `object_at` takes
    // both, and answers the version a uri names.
    let object = source
        .object_at(id)
        .ok_or_else(|| format!("`{id}` is not an object of this package"))?;
    let root = object.as_reflect();
    let key = root.type_info().type_key;
    let title = identity::title_of_reflect(root).unwrap_or_default().to_string();

    // A frame is collared where its trajectory is.
    let followed;
    let trajectory: &dyn Reflect = match key {
        TRAJECTORY => root,
        FRAME => {
            let Some(FieldValue::Object(reference)) = root.field("Trajectory") else {
                return Err(format!("`{title}` is a wellbore frame that names no trajectory"));
            };
            let uri = crate::follow::address_of(reference).ok_or_else(|| {
                format!("`{title}` is a wellbore frame whose trajectory reference carries no uuid")
            })?;
            followed = source.object_at(&uri).ok_or_else(|| {
                format!("`{title}` is a wellbore frame whose trajectory {uri} is not in this package")
            })?;
            followed.as_reflect()
        }
        other => {
            return Err(format!(
                "`{id}` is a `{other}`: only a wellbore trajectory or a wellbore frame has a head to project"
            ));
        }
    };

    let [x, y, _] = well::head_of(trajectory, source).map_err(|why| format!("{why}"))?;
    let crs = crs::of(trajectory, source).unwrap_or(None);
    let has_geometry = crate::geometry::field_object(trajectory, "Geometry").is_some();
    let (xy, system) = place(x, y, crs.as_ref(), has_geometry, placement)?;
    Ok((
        WellHead {
            id: id.to_string(),
            title,
            xy,
        },
        system,
    ))
}

/// `head_of`'s projected `x`/`y` into a placement — the table of the module.
fn place(
    x: f64,
    y: f64,
    crs: Option<&Crs>,
    has_geometry: bool,
    placement: Placement,
) -> std::result::Result<([f64; 2], System), String> {
    let epsg = crs.and_then(|crs| crs.projected.epsg());
    let uom = crs.and_then(|crs| crs.horizontal_uom.clone());
    match placement {
        Placement::Projected => Ok(([x, y], System { epsg, uom })),
        Placement::Metres => {
            // No unit stated is the schema's own default (metres), as in
            // `Crs::to_metres`; one stated and unknown to the table refuses.
            let convert = |value: f64| match &uom {
                Some(unit) => {
                    units::metres(value, unit).map_err(|why| format!("the horizontal unit `{unit}`: {why}"))
                }
                None => Ok(value),
            };
            Ok(([convert(x)?, convert(y)?], System { epsg, uom: None }))
        }
        Placement::Local => match crs {
            Some(crs) if has_geometry => {
                let [lx, ly, _] = crs.from_projected([x, y, 0.0]);
                Ok(([lx, ly], System { epsg, uom }))
            }
            _ => Ok(([x, y], System { epsg, uom })),
        },
    }
}
