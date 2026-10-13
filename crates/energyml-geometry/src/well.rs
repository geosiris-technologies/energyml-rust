//! Wells whose only spatial anchor is a reference point.
//!
//! RESQML 2.2 makes `Geometry` optional on a `WellboreTrajectoryRepresentation`
//! and puts the obligation elsewhere — *"BUSINESS RULE: For purposes of the
//! trajectory the MdDatum within the MdInterval is mandatory"* (Technical
//! Reference Guide §12.17). Real packages use exactly that: the BRGM export
//! carries 120 trajectories that are nothing but an `MdInterval` and a
//! [`Datum`] pointing at a fully georeferenced `ReferencePointInACrs`, plus a
//! `WellboreFrameRepresentation` per well holding the measured depths its logs
//! hang on.
//!
//! # The assumption, stated
//!
//! A trajectory with a measured-depth interval and a wellhead, and **no other
//! spatial information anywhere**, is evaluated as a **vertical** well: down
//! from the reference point, one metre of depth per metre of measured depth.
//! That is an assumption, not arithmetic — a deviated well whose geometry was
//! simply not exported comes out wrong. It is taken because (a) it is the only
//! evaluation the data permits, (b) the producers that write this shape write
//! it for shallow vertical boreholes, and (c) the alternative — refusing 120
//! wells that carry everything needed to place them — helps nobody. A
//! trajectory that hints at a real shape elsewhere (`WitsmlTrajectory`,
//! `ParentIntersection`) is refused by name instead.
//!
//! # Two conventions, and why they do not clash
//!
//! A [`WellboreFrameRepresentation`] takes whichever of the two shapes its
//! trajectory has. When the trajectory carries a `Geometry`, the frame's nodes
//! are that parametric line evaluated at the frame's measured depths, and they
//! come out in the line's **local CRS as stored**, like every other
//! representation in this crate. When it carries none, both the trajectory and
//! the frame go through `Anchor`, which normalises to **z-up** because a
//! `ReferencePointInACrs` has no local CRS to defer to — and which **places** a
//! `ReferencePointInALocalEngineeringCompoundCrs` through the compound it is
//! written in, so that the two eml 2.3 datums come out in the same
//! convention: projected x and y, an elevation in metres.
//!
//! So z means different things in the two cases — but never within one
//! package, and never between a frame and the trajectory it hangs on, which is
//! the only pairing that has to line up on screen.
//!
//! [`WellboreFrameRepresentation`]: https://docs.energistics.org
//!
//! [`Datum`]: https://docs.energistics.org

use energyml_core::value::Reflect;

use crate::error::{Error, Result};
use crate::geometry::{Patch, Topology};
use crate::points::Point;
use crate::source::ValueSource;
use crate::values::{self, scalar_f64};
use energyml_domain::units;

/// A no-`Geometry` trajectory: the vertical segment from `MdMin` to `MdMax`
/// below the reference point.
pub(crate) fn datum_trajectory(
    representation: &dyn Reflect,
    source: &dyn ValueSource,
    at: &str,
) -> Result<Vec<Patch>> {
    // Anything that says the shape lives elsewhere makes "vertical" a guess
    // rather than the only reading, so those are refused by name.
    for field in ["WitsmlTrajectory", "ParentIntersection"] {
        if representation
            .field(field)
            .is_some_and(|value| value.as_object().is_some())
        {
            return Err(Error::unsupported(
                "a wellbore trajectory whose shape is deferred to another object",
                format!("{at}.{field}"),
            ));
        }
    }
    let anchor = Anchor::of(representation, source, at)?;
    let top = anchor.at(anchor.md_min);
    let bottom = anchor.at(anchor.md_max);
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points: vec![top, bottom],
        topology: Topology::Polylines(vec![2]),
        closed: Vec::new(),
    }])
}

/// Where a trajectory is collared: `[x, y, elevation]` of the datum its
/// `MdInterval` names, the elevation **z-up** whatever the file's vertical CRS
/// says — in the coordinates of that datum's CRS, and for a datum written in a
/// local engineering compound, placed through that compound into its
/// projected system, the elevation in metres.
///
/// The reading that draws the well, made public for what tabulates it
/// (`crate::wells`): a second reader of the datum would be a second
/// answer to *where is this well*, in the two shapes the releases give it.
pub fn head_of(trajectory: &dyn Reflect, source: &dyn ValueSource) -> Result<[f64; 3]> {
    let anchor = Anchor::of(trajectory, source, "WellboreTrajectoryRepresentation")?;
    Ok([anchor.x, anchor.y, anchor.elevation])
}

/// **A trajectory's own line, evaluable at any measured depth** — the reading
/// [`frame`] places a frame's nodes with, for what needs a depth between two
/// stations (a computed top).
///
/// Two refusals, and both are the file's silence rather than a gap here:
///
/// - **no `Geometry`**: the only placement left is the vertical reading from
///   the datum this module documents — an assumption, and one written in
///   placed, z-up coordinates where a geometry's line is local to its CRS. A
///   number computed on it would carry the assumption without saying so;
/// - **no `ControlPointParameters`**: the knots are then 0, 1, 2 … (what the
///   schema says to assume), and no parameter along the line is a measured
///   depth. The contract between a trajectory and its frames is that the
///   parameter *is* the MD (the contract `energyml-build` relies on).
pub fn path_of(trajectory: &dyn Reflect, source: &dyn ValueSource) -> Result<crate::parametric::Path> {
    let at = "WellboreTrajectoryRepresentation";
    let Some(geometry) = crate::geometry::field_object(trajectory, "Geometry") else {
        return Err(Error::unsupported(
            "a wellbore trajectory with no `Geometry` — its only placement is the vertical reading from its datum, an assumption",
            at,
        ));
    };
    let path = crate::parametric::Path::of(geometry, source)?;
    if path.knots().is_none() {
        return Err(Error::malformed(
            at,
            "a line that states no `ControlPointParameters`: its knots are then 0, 1, 2 …, and no parameter along it is a measured depth",
        ));
    }
    Ok(path)
}

/// A `WellboreFrameRepresentation`: its measured depths, placed on the well.
///
/// The frame is where the logs hang — one property value per node — so
/// evaluating it is what makes well data drawable at all. The nodes come out
/// as one polyline, top to bottom, in the order the frame stores them.
pub(crate) fn frame(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let at = "WellboreFrameRepresentation";
    let Some(node_md) = crate::geometry::field_object(representation, "NodeMd") else {
        return Err(Error::malformed(at, "a frame with no `NodeMd`"));
    };
    let mds = values::read_doubles(node_md, source, &format!("{at}.NodeMd"))?;
    if mds.is_empty() {
        return Err(Error::malformed(at, "a frame with no measured depth"));
    }

    let Some(trajectory_uri) = reference_of(representation, "Trajectory") else {
        return Err(Error::malformed(at, "a frame that names no trajectory"));
    };
    let Some(trajectory) = source.object_at(&trajectory_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the trajectory {trajectory_uri}"
        ))));
    };
    let trajectory = trajectory.as_reflect();

    // Two ways to place a measured depth, and which one applies is decided by
    // the trajectory, not by the frame.
    let points: Vec<Point> = match crate::geometry::field_object(trajectory, "Geometry") {
        // The trajectory has a shape of its own. The parameter of its
        // parametric line **is** the measured depth — RESQML says so, and the
        // corpus confirms it: `acd2cdcf`'s parameters run 0…1000 and its
        // frames sample 0, 200 … 1000 of the same well. So the frame's nodes
        // are the line evaluated at its own MDs, in the line's local CRS,
        // which is the space the trajectory itself comes out in — a log has to
        // land *on* the well path, and that is the property that matters.
        //
        // The frame states no unit for `NodeMd`; the only MD unit in reach is
        // the trajectory's, and both describe depths along the same wellbore,
        // so they are read as being in it. A frame that meant something else
        // would have no way to say so.
        Some(geometry) => {
            let line = crate::parametric::Line::single(geometry, source, at)?;
            mds.iter().map(|md| line.at(*md)).collect()
        }
        // No shape anywhere: the vertical reading this module documents, from
        // the wellhead down. Its points are z-up by construction, matching
        // `datum_trajectory`, so frame and well still agree with each other.
        None => {
            let anchor = Anchor::of(trajectory, source, at)?;
            mds.iter().map(|md| anchor.at(*md)).collect()
        }
    };
    let count = points.len() as u32;
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points,
        topology: Topology::Polylines(vec![count]),
        closed: Vec::new(),
    }])
}

/// How many points sample a deviated trajectory's head portion.
///
/// Enough to show a bend without pretending to a precision the segment does
/// not have — this portion is already drawn differently from the frame's own
/// nodes (see the front end), so a coarse sample is honest rather than a shortcut.
const HEAD_SAMPLES: usize = 17;

/// **The portion of a frame's trajectory from measured depth 0 to its first
/// node** — the span a CSV import kept no stratigraphic unit for (on
/// `forages-unites.epc`, 2 654 of 2 699 frames start below ground, median
/// 14.5 m, because the units their MD interval covers do not start at
/// the wellhead).
///
/// **No object in the package describes this interval.** A frame's own nodes
/// start wherever the first one was measured, so what lies above it is read,
/// never invented: followed along the trajectory's own shape where the
/// trajectory's stated interval reaches MD 0, and as the straight segment to
/// the datum otherwise — the datum being where MD 0 *is*, by the business
/// rule that makes `MdInterval.Datum` mandatory (TRG §12.17, quoted at the
/// top of this module). Both endings are read by the exact evaluation
/// [`frame`] uses for its own first node, so the two never disagree about
/// where that node is.
///
/// `Ok(None)` when there is nothing missing — the frame already starts at or
/// below MD 0 — and a named [`Error`] when neither the trajectory's shape nor
/// its datum can be read, rather than a guess.
pub fn head_portion(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Option<Vec<Point>>> {
    let at = "WellboreFrameRepresentation";
    let Some(node_md) = crate::geometry::field_object(representation, "NodeMd") else {
        return Err(Error::malformed(at, "a frame with no `NodeMd`"));
    };
    let mds = values::read_doubles(node_md, source, &format!("{at}.NodeMd"))?;
    let Some(&first_md) = mds.first() else {
        return Err(Error::malformed(at, "a frame with no measured depth"));
    };
    if first_md <= 0.0 {
        return Ok(None);
    }

    let Some(trajectory_uri) = reference_of(representation, "Trajectory") else {
        return Err(Error::malformed(at, "a frame that names no trajectory"));
    };
    let Some(trajectory) = source.object_at(&trajectory_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the trajectory {trajectory_uri}"
        ))));
    };
    let trajectory = trajectory.as_reflect();

    // One read answers both questions at once: `Anchor::of` places the datum
    // — MD 0 by definition — and carries the trajectory's own `MdMin` beside
    // it, so *where is MD 0* and *does the trajectory's shape reach it* come
    // from the same fact rather than two readings that could disagree.
    let anchor = Anchor::of(trajectory, source, "WellboreTrajectoryRepresentation");
    let geometry = crate::geometry::field_object(trajectory, "Geometry");

    let points = match (geometry, anchor.as_ref()) {
        (Some(geometry), Ok(anchor)) if anchor.md_min <= 0.0 => {
            // The trajectory's own line reaches MD 0: followed, not guessed
            // at as a straight line through a well that may be deviated.
            let line = crate::parametric::Line::single(geometry, source, at)?;
            (0..HEAD_SAMPLES)
                .map(|i| first_md * (i as f64) / (HEAD_SAMPLES as f64 - 1.0))
                .map(|md| line.at(md))
                .collect()
        }
        (_, Ok(anchor)) => {
            // Either no `Geometry` at all, or one whose stated interval does
            // not reach MD 0: the datum is where MD 0 *is*, so the segment is
            // straight from there — never an extrapolation of a spline past
            // the range the file states it valid for.
            vec![anchor.at(0.0), anchor.at(first_md)]
        }
        (_, Err(_)) => {
            return Err(Error::unsupported(
                "a frame's head portion: neither the trajectory's own shape nor its datum can be read",
                at,
            ));
        }
    };
    Ok(Some(points))
}

/// The measured-depth bounds, in the two shapes they have worn.
///
/// The released RESQML 2.2 groups them into an `MdInterval`; **RESQML 2.0.1 and
/// the 2.2 development drops put the same four values flat on the trajectory**
/// — `StartMd`, `FinishMd`, `MdUom`, `MdDatum`. One concept, two spellings,
/// handled the way this workspace handles every other respelling: a list of
/// names beside the code that reads them, never a match on version.
const MD_INTERVAL: &str = "MdInterval";
const MD_MIN: &[&str] = &["MdMin", "StartMd"];
const MD_MAX: &[&str] = &["MdMax", "FinishMd"];
const MD_UOM: &[&str] = &["Uom", "MdUom"];
const MD_DATUM: &[&str] = &["Datum", "MdDatum"];

/// The datum object, under its three names — `energyml_domain::crs`'s list,
/// never a copy of it, so that the CRS a well is given and the point it is
/// drawn from cannot disagree on what a datum is.
///
/// `MdDatum` in 2.0.1 and the drops; `ReferencePointInACrs` and
/// `ReferencePointInALocalEngineeringCompoundCrs` in 2.2. The same thing — a
/// georeferenced point a measured depth is counted from — and they hold their
/// coordinates differently, which [`Anchor::coordinates`] reconciles and
/// [`Anchor::of`] places.
const DATUM_TYPES: &[&str] = energyml_domain::crs::DATUM_TYPES;

/// Where a well is, and how far down it runs.
struct Anchor {
    x: f64,
    y: f64,
    /// The wellhead, as an elevation — positive **up**, whatever the file's
    /// vertical CRS says, so every point this module emits is z-up.
    elevation: f64,
    /// Metres per unit of measured depth, from the interval's `Uom`.
    md_to_metres: f64,
    md_min: f64,
    md_max: f64,
}

impl Anchor {
    /// The datum's position, in the two shapes the releases give it.
    ///
    /// 2.2 splits it — `HorizontalCoordinates{Coordinate1,2}` beside a
    /// `VerticalCoordinate` — while 2.0.1 and the drops keep one
    /// `Location.Point3d` with all three. Same point, and the vertical one is
    /// returned in the file's own sense; [`vertical_sense`] says which way that
    /// is.
    fn coordinates(datum: &dyn Reflect, at: &str) -> Result<(f64, f64, f64)> {
        if let Some(horizontal) = crate::geometry::field_object(datum, "HorizontalCoordinates") {
            let x = scalar_f64(horizontal, "Coordinate1")
                .ok_or_else(|| Error::malformed(at, "a reference point with no `Coordinate1`"))?;
            let y = scalar_f64(horizontal, "Coordinate2")
                .ok_or_else(|| Error::malformed(at, "a reference point with no `Coordinate2`"))?;
            let vertical = scalar_f64(datum, "VerticalCoordinate")
                .ok_or_else(|| Error::malformed(at, "a reference point with no `VerticalCoordinate`"))?;
            return Ok((x, y, vertical));
        }
        let point = crate::geometry::field_object(datum, "Location")
            .and_then(|location| crate::geometry::field_object(location, "Point3d"))
            .ok_or_else(|| {
                Error::malformed(at, "a datum with neither horizontal coordinates nor a `Location`")
            })?;
        let x = scalar_f64(point, "Coordinate1")
            .ok_or_else(|| Error::malformed(at, "a datum location with no `Coordinate1`"))?;
        let y = scalar_f64(point, "Coordinate2")
            .ok_or_else(|| Error::malformed(at, "a datum location with no `Coordinate2`"))?;
        let z = scalar_f64(point, "Coordinate3")
            .ok_or_else(|| Error::malformed(at, "a datum location with no `Coordinate3`"))?;
        Ok((x, y, z))
    }

    /// The point at a measured depth, z-up.
    fn at(&self, md: f64) -> Point {
        [self.x, self.y, self.elevation - md * self.md_to_metres]
    }

    /// Read the anchor of a trajectory: its `MdInterval` and the reference
    /// point the interval's `Datum` names — one of [`DATUM_TYPES`].
    fn of(trajectory: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Self> {
        // Grouped in an `MdInterval` (2.2) or flat on the trajectory itself
        // (2.0.1 and the drops). Falling back to the trajectory rather than
        // refusing is what makes one reader serve both.
        let interval = crate::geometry::field_object(trajectory, MD_INTERVAL).unwrap_or(trajectory);
        let (Some(md_min), Some(md_max)) = (scalar_any(interval, MD_MIN), scalar_any(interval, MD_MAX))
        else {
            // The spec's own business rule makes the datum mandatory, so a
            // trajectory with neither geometry nor depth bounds has no spatial
            // content at all.
            return Err(Error::unsupported(
                "a wellbore trajectory with neither `Geometry` nor measured-depth bounds (`MdInterval`, or `StartMd`/`FinishMd`)",
                at,
            ));
        };
        if md_min > md_max {
            // "The MdMin must be less than the MdMax within the MdInterval" —
            // the spec's rule, and a violated one means the interval cannot be
            // trusted to point down.
            return Err(Error::malformed(
                at,
                format!("MdMin {md_min} is greater than MdMax {md_max}"),
            ));
        }
        let uom = string_any(interval, MD_UOM).unwrap_or_else(|| "m".to_string());
        let md_to_metres = units::metres(1.0, &uom).map_err(Error::Domain)?;

        let Some(datum_uri) = reference_any(interval, MD_DATUM) else {
            return Err(Error::malformed(at, "depth bounds with no datum"));
        };
        let placed = place_datum(&datum_uri, source, at)?;
        Ok(Self {
            x: placed.x,
            y: placed.y,
            elevation: placed.elevation,
            md_to_metres,
            md_min,
            md_max,
        })
    }
}

/// A measured-depth datum, placed: projected `x` and `y`, and an elevation in
/// the convention of this module — see [`Anchor`].
pub(crate) struct PlacedDatum {
    pub(crate) x: f64,
    pub(crate) y: f64,
    /// Positive **up**.
    pub(crate) elevation: f64,
    /// The compound the datum is written in, when it is a
    /// `ReferencePointInALocalEngineeringCompoundCrs` — what placed it.
    pub(crate) compound: Option<energyml_domain::Crs>,
}

/// Where a datum is, read once for every well that counts its depths from one:
/// a RESQML trajectory's `MdInterval.Datum` ([`Anchor`]) and a WITSML
/// trajectory's `DefaultMdDatum` (`crate::stations`). Two readings of *where is
/// this datum* would be two answers.
pub(crate) fn place_datum(datum_uri: &str, source: &dyn ValueSource, at: &str) -> Result<PlacedDatum> {
    let Some(datum) = source.object_at(datum_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the datum {datum_uri}"
        ))));
    };
    let datum = datum.as_reflect();
    let type_key = datum.type_info().type_key;
    if !DATUM_TYPES.contains(&type_key) {
        return Err(Error::unsupported(
            crate::geometry::leak(type_key),
            format!("{at}.Datum"),
        ));
    }

    let (x, y, vertical) = Anchor::coordinates(datum, at)?;

    // **A point in a compound is a local point, and is placed.** Its
    // coordinates are counted along the axes of the compound its `Crs`
    // names (common TRG 2.3 §13.25 p. 371), so where the well stands is the
    // compound's placement of them: origin, azimuth, and a vertical axis
    // that may count down from a datum that counts up. The result is what
    // every point of this module is — projected x and y, an elevation — and
    // the elevation goes to metres to meet the measured depths.
    // `energyml_domain::crs::of` gives such a well the compound's identity
    // and none of its offsets, so nothing is placed twice.
    if let Some(compound) = energyml_domain::crs::compound_of_datum(datum, source).map_err(Error::Domain)? {
        if compound.is_time() {
            // A measured depth is a length; a datum in a time system has
            // no depth to count it from without a velocity model.
            return Err(Error::unsupported(
                "a measured-depth datum placed in a time CRS",
                format!("{at}.Datum"),
            ));
        }
        let [x, y, up] = compound.to_projected([x, y, vertical]);
        let elevation = match compound.vertical_uom.as_deref() {
            Some(unit) => units::metres(up, unit).map_err(Error::Domain)?,
            None => up,
        };
        return Ok(PlacedDatum {
            x,
            y,
            elevation,
            compound: Some(compound),
        });
    }

    let elevation = match vertical_sense(datum, source, at)? {
        VerticalSense::Up => vertical,
        VerticalSense::Down => -vertical,
    };
    Ok(PlacedDatum {
        x,
        y,
        elevation,
        compound: None,
    })
}

enum VerticalSense {
    Up,
    Down,
}

/// Which way the datum's `VerticalCoordinate` points.
///
/// Two sources can answer, and the corpus proves they can disagree: the BRGM
/// export names **EPSG:5714** — "MSL height", positive up, and its wellheads
/// sit at +30 to +40 m, which is right for the Paris region — while writing
/// `Direction: down` on the same object, which would put every wellhead under
/// the sea. The EPSG code *is* the convention (5714 = height, 5715 = depth),
/// so when one of those two codes is named it wins; the `Direction` element is
/// only believed when no code says otherwise.
fn vertical_sense(datum: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<VerticalSense> {
    // 2.2 names a `VerticalCrs` on the datum itself. 2.0.1 and the drops give
    // the datum a `Location` whose `LocalCrs` carries the sense, as a
    // `ZIncreasingDownward` flag — so that is the second place to look, and
    // looking there is what lets a drop's wells stand the right way up.
    let vertical_crs = reference_of(datum, "VerticalCrs")
        .or_else(|| {
            crate::geometry::field_object(datum, "Location")
                .and_then(|location| reference_of(location, "LocalCrs"))
        })
        .and_then(|uri| source.object_at(&uri));
    let Some(vertical_crs) = vertical_crs else {
        // No CRS to consult: a bare vertical coordinate has no stated sense,
        // and up-vs-down is the whole well.
        return Err(Error::unsupported(
            "a reference point whose vertical CRS is not in the package",
            at,
        ));
    };
    let crs = vertical_crs.as_reflect();

    if let Some(code) = epsg_of(crs) {
        match code {
            5714 => return Ok(VerticalSense::Up),
            5715 => return Ok(VerticalSense::Down),
            _ => {}
        }
    }
    // A local CRS states it as a boolean rather than a word.
    if let Some(energyml_core::value::FieldValue::Scalar(energyml_core::value::Scalar::Bool(down))) =
        crs.field("z_increasing_downward")
    {
        return Ok(if down {
            VerticalSense::Down
        } else {
            VerticalSense::Up
        });
    }
    match string_of(crs, "Direction").as_deref() {
        Some("up") => Ok(VerticalSense::Up),
        Some("down") => Ok(VerticalSense::Down),
        _ => Err(Error::unsupported("a vertical CRS that states no direction", at)),
    }
}

/// The EPSG code a CRS object carries, wherever its version put it.
fn epsg_of(crs: &dyn Reflect) -> Option<i64> {
    fn walk(object: &dyn Reflect, depth: u8) -> Option<i64> {
        if depth > 3 {
            return None;
        }
        for candidate in ["EpsgCode", "Epsg", "Code"] {
            if let Some(code) = values::scalar_i64(object, candidate)
                && code > 0
            {
                return Some(code);
            }
        }
        for field in object.type_info().fields {
            if let Some(energyml_core::value::FieldValue::Object(child)) = object.field(field.name)
                && let Some(code) = walk(child, depth + 1)
            {
                return Some(code);
            }
        }
        None
    }
    walk(crs, 0)
}

/// The uri a DOR-valued field points at — the full identity, version included
/// when the reference declares one — so the object it names is the object read
/// (`ObjectSource::object_at`), never another version sharing its uuid.
/// The first of these fields that holds a number.
fn scalar_any(object: &dyn Reflect, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| scalar_f64(object, name))
}

/// The first of these fields that holds a string.
fn string_any(object: &dyn Reflect, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| string_of(object, name))
}

/// The first of these fields that holds a reference.
fn reference_any(object: &dyn Reflect, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| reference_of(object, name))
}

pub(crate) fn reference_of(object: &dyn Reflect, field: &str) -> Option<String> {
    let energyml_core::value::FieldValue::Object(reference) = object.field(field)? else {
        return None;
    };
    crate::follow::address_of(reference)
}

/// Whether two uris name the same object: the same uuid, and object versions
/// that do not contradict each other — an absent version means "the latest",
/// so it matches any (`ObjectKey::same_object`). Text that is no uri is
/// compared as it stands.
pub(crate) fn same_uri(a: &str, b: &str) -> bool {
    use energyml_core::identifiers::EtpUri;
    match (EtpUri::parse(a), EtpUri::parse(b)) {
        (Ok(a), Ok(b)) => {
            a.uuid == b.uuid
                && match (&a.object_version, &b.object_version) {
                    (Some(x), Some(y)) => x == y,
                    _ => true,
                }
        }
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// A scalar string field.
fn string_of(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        energyml_core::value::FieldValue::Scalar(scalar) => scalar.as_str().map(|s| s.to_string()),
        _ => None,
    }
}
