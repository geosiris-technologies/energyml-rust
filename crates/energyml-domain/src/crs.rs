//! One coordinate reference system, whichever version wrote it.
//!
//! # Why this is a view and not a structure
//!
//! The obvious design is a pivot struct that every version converts into and
//! out of. It does not survive contact with the schemas, because two concepts
//! do not survive the trip back:
//!
//! - **Axis order.** RESQML 2.0.1 has `ProjectedAxisOrder: AxisOrder2d`, six
//!   values. eml 2.3 has a *pair* of `AxisDirectionKind`, about thirty values
//!   including `display-up` and `column-positive`. 2.3 → 2.0.1 loses
//!   information; a round-trippable pivot would have to invent it.
//! - **Time versus depth.** 2.0.1 makes it the *type* — `LocalTime3dCrs` and
//!   `LocalDepth3dCrs` are different objects. eml 2.3 makes it a *flag*,
//!   `VerticalAxis.IsTime`, on one type.
//!
//! So this reads: [`Crs`] answers questions, and says `None` where a version
//! cannot answer. Writing a CRS back stays the business of the version-specific
//! types, where nothing has to be invented.
//!
//! # What differs between versions
//!
//! | Concept | RESQML 2.0.1 | eml 2.3 |
//! |---|---|---|
//! | horizontal offsets | `XOffset`, `YOffset` | `OriginProjectedCoordinate1/2`, on the **2d CRS** |
//! | vertical offset | `ZOffset`, along the local axis | `OriginVerticalCoordinate`, on the compound, **in the base vertical CRS** |
//! | rotation | `ArealRotation`, clockwise from the projected Y axis | `Azimuth` + `AzimuthReference`, clockwise from north |
//! | z downward | `ZIncreasingDownward: bool` | `VerticalAxis.Direction: up`/`down` — not the `VerticalCrs`'s |
//! | projected system | inline choice | **two references away** |
//! | vertical system | inline choice | a referenced top-level object |
//! | horizontal unit | `ProjectedUom` | `HorizontalAxes.Uom` |
//! | vertical unit | `VerticalUom` | `VerticalAxis.Uom` — the `VerticalCrs`'s `uom` is the datum's |
//!
//! # The local axis and the datum are two statements (24/09/2026)
//!
//! eml 2.3 says *how the local z counts* and *what it counts from* in two
//! objects, and they may disagree without either being wrong. The compound's
//! `VerticalAxis` (common TRG 2.3 §12.34 p. 338: `Direction`, `Uom`, `IsTime`)
//! is the local axis: it governs the direction, the unit and the time flag of
//! every stored z. The referenced `VerticalCrs` (§12.35 p. 339) is the
//! **datum** — the zero, and the direction and unit that zero is counted in.
//! `OriginVerticalCoordinate` is written in the second, not the first: *"Vertical
//! coordinate of the origin of the local engineering CRS in the base vertical
//! CRS (consequently in the uom of the base vertical CRS)"* (CRS.xsd l. 517–520;
//! common TRG 2.3 §12.20 p. 322).
//!
//! Until 24/09 this reader let the `VerticalCrs` overwrite the axis's direction
//! and unit, and applied the origin along the local axis. Nothing in the corpus
//! could see it — in every 2.2 package measured the two directions agree — and
//! Lutetia's depth compound (axis `down`, NGF-IGN69 `up`, origin 35 m) would
//! have been read `up`, with an elevation of −(35 + z) where the file says
//! 35 − z (found on the synthetic reference model, ADR-0018).
//!
//! RESQML 2.0.1 has one sense for both: `ZIncreasingDownward` *"must correspond
//! to the axis orientation of the vertical CRS"* when that CRS is known (RESQML
//! TRG 2.0.1 §2.1 p. 11), so its `ZOffset` is along the local axis and read as
//! it stands.
//!
//! The structural difference that matters most: 2.0.1 is **one self-contained
//! object**, eml 2.3 is **three objects joined by references**. That is why
//! reading takes a [`ObjectSource`] rather than being a pure function — and
//! why an ETP dataspace will work here exactly as an EPC file does.
//!
//! Reading is by *shape*, not by version: whichever fields are present win. A
//! version this crate has never seen reads correctly to the extent it spells
//! things the way its predecessors did, and says `None` rather than guessing
//! where it does not.

use energyml_core::identity;
use energyml_core::measure;
use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::source::ObjectSource;
use crate::units;

/// Which system a projected or vertical component names.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CrsKind {
    /// An EPSG code — the only form this crate can act on.
    Epsg(u64),
    /// A definition inline in GML. Present in eml 2.0/2.1/2.2, dropped in 2.3.
    Gml,
    /// A definition as a WKT string.
    Wkt(String),
    /// Named by an authority other than EPSG.
    LocalAuthority(String),
    /// The file says explicitly that it does not know.
    Unknown,
    /// No component of this kind was named at all.
    Absent,
}

impl CrsKind {
    /// The EPSG code, when there is one.
    pub fn epsg(&self) -> Option<u64> {
        match self {
            CrsKind::Epsg(code) => Some(*code),
            _ => None,
        }
    }
}

/// Which way the vertical axis counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerticalDirection {
    /// Increasing values go down — depths. RESQML's `ZIncreasingDownward`.
    Down,
    /// Increasing values go up — elevations.
    Up,
}

/// What the third coordinate measures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerticalQuantity {
    /// A length: a depth or an elevation.
    Length,
    /// A time: a seismic travel time. `LocalTime3dCrs`, or `IsTime` in 2.3.
    Time,
}

/// A coordinate reference system, read the same way in every version.
#[derive(Clone, Debug, PartialEq)]
pub struct Crs {
    /// The uuid of the object this was read from.
    pub uuid: String,
    /// Its type name, for a caller that wants to label it.
    pub kind: &'static str,

    /// Offset of the local origin along the first projected axis, in
    /// [`horizontal_uom`](Crs::horizontal_uom).
    pub x_offset: f64,
    /// Offset along the second projected axis.
    pub y_offset: f64,
    /// Offset of the local origin along the **local** vertical axis — counted
    /// the way [`vertical_direction`](Crs::vertical_direction) counts, in
    /// [`vertical_uom`](Crs::vertical_uom) — from the zero of the vertical
    /// datum.
    ///
    /// RESQML 2.0.1's `ZOffset` is exactly that. eml 2.3's
    /// `OriginVerticalCoordinate` is not: it is written in the base vertical
    /// CRS, its direction and its unit, and [`read_with`] converts it — see
    /// the module doc.
    pub z_offset: f64,

    /// Rotation of the local axes, **anticlockwise from the first projected
    /// axis, in radians** — this view's own convention, which neither release
    /// writes.
    ///
    /// Both releases state a **clockwise** angle measured from the projected Y
    /// axis: RESQML 2.0.1's `ArealRotation` (*"A positive value indicates a
    /// clockwise rotation from the projected Y axis"*, RESQML TRG 2.0.1 §2.1
    /// p. 10) and eml 2.3's `Azimuth` from grid north. [`read_with`] negates
    /// both on the way in.
    pub areal_rotation: f64,

    /// Which way the third, **local** coordinate counts.
    ///
    /// eml 2.3's `VerticalAxis.Direction`, never the referenced
    /// `VerticalCrs`'s: that one says which way the datum counts, which is a
    /// different fact (common TRG 2.3 §12.34 p. 338, §12.35 p. 339).
    pub vertical_direction: VerticalDirection,
    /// Whether the third coordinate is a length or a time.
    pub vertical_quantity: VerticalQuantity,

    /// The unit horizontal coordinates are written in, as the file spells it.
    ///
    /// `None` when the CRS does not say. **Not** an invitation to assume
    /// metres — see [`Crs::to_metres`].
    pub horizontal_uom: Option<String>,
    /// The unit the third coordinate is written in.
    ///
    /// On an eml 2.3 length system, `VerticalAxis.Uom` — the unit the stored z
    /// is in — and never the `VerticalCrs`'s `uom`, which is its datum's.
    ///
    /// On a **time** system this is not the time unit: RESQML 2.0.1's
    /// `LocalTime3dCrs` carries `VerticalUom` for the vertical *reference* —
    /// the `ZOffset`, a length — and states the axis unit separately in
    /// `TimeUom`. Ask [`Crs::vertical_axis_uom`] rather than this field.
    pub vertical_uom: Option<String>,
    /// The unit of the time axis, when the system has one.
    ///
    /// RESQML 2.0.1 puts it in `TimeUom`, beside a `VerticalUom` that is a
    /// length. eml 2.3 has one `VerticalAxis.Uom` that *is* the time unit when
    /// `IsTime` is set, so this stays `None` there and
    /// [`Crs::vertical_axis_uom`] falls back correctly.
    pub time_uom: Option<String>,

    /// The projected system the local one sits in.
    pub projected: CrsKind,
    /// The vertical system.
    pub vertical: CrsKind,
}

/// A point in the CRS's own axes and units.
pub type Point = [f64; 3];

impl Crs {
    /// Whether this is one half of a system rather than a whole one.
    ///
    /// eml 2.3 promoted the parts of a coordinate system to top-level objects:
    /// a package contains `LocalEngineering2dCrs` and `VerticalCrs` objects
    /// alongside the `LocalEngineeringCompoundCrs` that joins them. Reading a
    /// part succeeds and gives a perfectly valid half — horizontal placement
    /// with no vertical, or the reverse — which is exactly the sort of thing
    /// that looks like a complete answer.
    ///
    /// A caller iterating over every CRS-shaped object in a package wants this;
    /// a caller following a representation's reference never sees one.
    pub fn is_component(&self) -> bool {
        !(self.kind.contains("3dCrs") || self.kind.contains("Compound"))
    }

    /// Whether this CRS moves a point at all.
    ///
    /// The common case in practice, and worth short-circuiting: most local
    /// CRSs in real files are the identity with a projected system attached.
    pub fn is_identity(&self) -> bool {
        self.x_offset == 0.0
            && self.y_offset == 0.0
            && self.z_offset == 0.0
            && self.areal_rotation == 0.0
            && self.vertical_direction == VerticalDirection::Up
    }

    /// Place a local point in the projected system.
    ///
    /// Rotation, then offset, and the vertical flip when the CRS counts
    /// downward — so the result is an elevation whichever way the file was
    /// written. Units are **not** touched: the result is in the CRS's own
    /// units, which [`to_metres`](Crs::to_metres) normalises.
    ///
    /// # The third coordinate
    ///
    /// `z_offset + z` is where the point sits counted along the local axis
    /// from the datum's zero — both terms count the same way, because
    /// [`read_with`] has put the eml 2.3 origin into the axis's sense. Flipping
    /// a downward count gives the **elevation above the vertical datum**, the
    /// one convention every caller draws in. With Lutetia's depth compound —
    /// axis `down`, NGF-IGN69 counting `up`, `OriginVerticalCoordinate` 35
    /// (CRS.xsd l. 517–520: *"in the base vertical CRS"*) — the offset along
    /// the axis is −35 and a local depth z lands at 35 − z.
    ///
    /// This is the local placement only. Going from here to another EPSG
    /// system is [`crate::reproject`].
    pub fn to_projected(&self, point: Point) -> Point {
        let (sin, cos) = self.areal_rotation.sin_cos();
        let [x, y, z] = point;
        [
            self.x_offset + x * cos - y * sin,
            self.y_offset + x * sin + y * cos,
            match self.vertical_direction {
                VerticalDirection::Down => -(self.z_offset + z),
                VerticalDirection::Up => self.z_offset + z,
            },
        ]
    }

    /// The exact inverse of [`Crs::to_projected`]: a **placed** point back
    /// into this CRS's local frame.
    ///
    /// What a point must go through before being **stored** in an object that
    /// references this CRS: RESQML geometry is written in the local frame, and
    /// every reader — this workspace's included — applies the placement on the
    /// way out. Storing already-placed coordinates applies the offsets, the
    /// rotation and the vertical flip a second time at every read — invisible
    /// on a CRS whose origin is (0, 0) and axis is Up, which is every corpus
    /// package, and wrong on a real one. The round trip
    /// `from_projected(to_projected(p)) == p` is pinned by a test with all
    /// three of an offset, a rotation and a Down axis, because that identity
    /// is the whole reason this function exists — and again on a compound
    /// whose axis and datum count in opposite directions, the case the
    /// conversion of `OriginVerticalCoordinate` in [`read_with`] is about.
    pub fn from_projected(&self, point: Point) -> Point {
        let [x, y, z] = point;
        let local_z = match self.vertical_direction {
            VerticalDirection::Down => -z - self.z_offset,
            VerticalDirection::Up => z - self.z_offset,
        };
        let (sin, cos) = self.areal_rotation.sin_cos();
        let (dx, dy) = (x - self.x_offset, y - self.y_offset);
        // The inverse rotation: the transpose of the rotation matrix.
        [dx * cos + dy * sin, -dx * sin + dy * cos, local_z]
    }

    /// Place a point *and* convert it to metres.
    ///
    /// The step that is easy to forget and impossible to notice: a file whose
    /// `ProjectedUom` is `ft` is a perfectly ordinary file, and reading it as
    /// metres puts the model 3.28 times too far from the origin with every
    /// coordinate still looking plausible.
    ///
    /// Errors rather than assuming when a unit is outside the table. A CRS
    /// that states no unit at all is taken at its word and left alone — that
    /// is the schema's own default, not a guess.
    pub fn to_metres(&self, point: Point) -> Result<Point> {
        if self.vertical_quantity == VerticalQuantity::Time {
            return Err(Error::unsupported(
                "converting a travel time to metres",
                format!("the CRS {} is a time system", self.uuid),
            ));
        }
        let [x, y, z] = self.to_projected(point);
        let horizontal = |value: f64| match &self.horizontal_uom {
            Some(unit) => units::metres(value, unit),
            None => Ok(value),
        };
        let vertical = match &self.vertical_uom {
            Some(unit) => units::metres(z, unit)?,
            None => z,
        };
        Ok([horizontal(x)?, horizontal(y)?, vertical])
    }

    /// The unit the third coordinate is actually written in.
    ///
    /// The one that has to be asked, because the two releases put it in
    /// different fields and getting it wrong is a factor of a thousand: ALWYN's
    /// seismic model is a 2.0.1 `LocalTime3dCrs` whose `VerticalUom` reads `m`
    /// — perfectly correct, it describes the `ZOffset` — while the axis itself
    /// is in `TimeUom`. Reading `vertical_uom` on a time system and handing it
    /// to a time conversion is how a millisecond becomes a metre.
    pub fn vertical_axis_uom(&self) -> Option<&str> {
        if self.is_time() {
            if let Some(unit) = self.time_uom.as_deref() {
                return Some(unit);
            }
        }
        self.vertical_uom.as_deref()
    }

    /// Whether the third coordinate is a **travel time** rather than a length.
    ///
    /// # Why a viewer has to ask
    ///
    /// A time-domain surface has no depth, and nothing in the file gives it one:
    /// the conversion needs a velocity model the package does not carry, and
    /// RESQML deliberately puts the time *kind* — two-way against one-way —
    /// outside the CRS (TRG §12.9 leaves it to the acquisition activity). So
    /// [`Crs::to_metres`] refuses, which is right.
    ///
    /// [`Crs::to_projected`] does **not** refuse, and that is the trap. It
    /// applies the offsets and the flip like any other axis and hands back a
    /// number, so a seismic horizon at 2 713 **milliseconds** goes into a scene
    /// as z = −2 713 and lands among depth objects at −2 713 **metres**. Nothing
    /// errors, and the two are within about ten percent of each other for real
    /// velocities — which is exactly the range in which a wrong picture is
    /// indistinguishable from a right one.
    ///
    /// A caller drawing several objects together therefore has to know which
    /// vertical domain each is in, and say so. This is that question.
    pub fn is_time(&self) -> bool {
        self.vertical_quantity == VerticalQuantity::Time
    }

    /// Whether the coordinates are already in metres, or say nothing.
    pub fn is_metric(&self) -> bool {
        let metric = |unit: &Option<String>| match unit {
            Some(unit) => unit == "m",
            None => true,
        };
        metric(&self.horizontal_uom) && metric(&self.vertical_uom)
    }
}

/// The CRS an object points at.
///
/// `None` when it names none, which is legal for an object carrying no
/// geometry of its own.
pub fn of(object: &dyn Reflect, source: &dyn ObjectSource) -> Result<Option<Crs>> {
    of_within(object, source, 0)
}

/// The hop bound is over **borrowed** systems: a frame defers to its
/// trajectory, a local grid to its parent, and a file may chain either.
fn of_within(object: &dyn Reflect, source: &dyn ObjectSource, hop: u8) -> Result<Option<Crs>> {
    let Some(named) = reference_in(object) else {
        if hop > 4 {
            return Ok(None);
        }
        // A **local grid** names no CRS because it holds no geometry: its
        // points are its parent's, subdivided, so its system is the parent's
        // and nowhere else. Without this hop such a grid comes out "no CRS" —
        // a refinement floating free of the model it refines, exported with no
        // way to place or reproject it.
        if let Some(crs) = from_parent_window(object, source, 0)? {
            return Ok(Some(crs));
        }
        // Geometry borrowed from a sibling: the system is borrowed with it.
        //
        // **A list of lenders, not only one**, since 08/09/2026. A
        // `GridConnectionSetRepresentation` names its grids in `Grid`, which is
        // a *list* of references — so the previous `FieldValue::Object` match
        // could never see it, and Volve's connection set of 601 560 points came
        // out `[no CRS]`, drawn where the file never put it. It looked like a
        // reading bug in the connection set: it was one hop short in the
        // system that places it.
        for field in BORROWED_GEOMETRY_FIELDS {
            let lenders: Vec<&dyn Reflect> = match object.field(field) {
                Some(FieldValue::Object(lender)) => vec![lender],
                Some(FieldValue::List(items)) => {
                    items.into_iter().filter_map(|item| item.as_object()).collect()
                }
                _ => continue,
            };
            // **Every lender must agree, or nothing is claimed.** Two grids in
            // two systems are two systems, and taking the first would be *a
            // guess wearing a number* — the phrase this file already uses, two
            // paragraphs down, about a 2.2 sub-representation drawing from
            // several supports. The same reasoning, the same answer.
            let mut agreed: Option<Crs> = None;
            let mut disagree = false;
            for lender in lenders {
                let Some(resolved) = source.object_of(lender) else {
                    continue;
                };
                let Some(crs) = of_within(resolved.as_reflect(), source, hop + 1)? else {
                    continue;
                };
                match &agreed {
                    None => agreed = Some(crs),
                    Some(first) if *first == crs => {}
                    Some(_) => {
                        disagree = true;
                        break;
                    }
                }
            }
            if !disagree && let Some(crs) = agreed {
                return Ok(Some(crs));
            }
        }
        // **A RESQML 2.2 sub-representation names its supports on its patches**
        // (TRG 2.2 §8.10 p. 247: `SubRepresentationPatch.SupportingRepresentation`
        // 1 → `AbstractRepresentation`), so the object-level field above never
        // sees them. The same rule as `Grid`: every patch's support must agree,
        // or nothing is borrowed. Found on the synthetic reference model: "Productive
        // reservoir — Chalk", a sub-representation of the Ypresian–Chalk grid,
        // had no system, and the scene refused it.
        if let Some(crs) = patch_supports_agree(object, source, hop)? {
            return Ok(Some(crs));
        }
        // No CRS reference anywhere — the shape of a RESQML 2.2 well: a
        // trajectory whose only spatial anchor is the `ReferencePointInACrs`
        // its `MdInterval.Datum` names. Georeferenced through the datum.
        return from_well_datum(object, source, 0);
    };
    let crs = source
        .object_of(named)
        .ok_or_else(|| Error::Missing(format!("the CRS {}", uri_or_uuid(named))))?;
    Ok(Some(read_with(crs.as_reflect(), source)?))
}

/// The system every `SubRepresentationPatch.SupportingRepresentation` agrees
/// on, or `None` — no patch, no readable support, or two supports in two
/// systems (a guess wearing a number, as for `Grid`).
fn patch_supports_agree(object: &dyn Reflect, source: &dyn ObjectSource, hop: u8) -> Result<Option<Crs>> {
    let patches: Vec<&dyn Reflect> = match object.field("SubRepresentationPatch") {
        Some(FieldValue::Object(patch)) => vec![patch],
        Some(FieldValue::List(items)) => items.into_iter().filter_map(|item| item.as_object()).collect(),
        _ => return Ok(None),
    };
    let mut agreed: Option<Crs> = None;
    for patch in patches {
        let Some(FieldValue::Object(support)) = patch.field("SupportingRepresentation") else {
            continue;
        };
        let Some(resolved) = source.object_of(support) else {
            continue;
        };
        let Some(crs) = of_within(resolved.as_reflect(), source, hop + 1)? else {
            continue;
        };
        match &agreed {
            None => agreed = Some(crs),
            Some(first) if *first == crs => {}
            Some(_) => return Ok(None),
        }
    }
    Ok(agreed)
}

/// The fields by which a representation **borrows another's geometry** — and
/// therefore the system that geometry is expressed in.
///
/// # Why this is a list and not two `if`s
///
/// A representation whose points are computed from another's is in the other's
/// coordinate system, always: it did not place anything itself. The pairs
/// today are a `WellboreFrameRepresentation` naming its `Trajectory`, and a
/// `SubRepresentation` or line lattice naming its `SupportingRepresentation`.
///
/// They are here together because they failed together, and the failure is
/// worth the paragraph. The geometry reader follows one of these references to
/// compute its points; the CRS lookup used to follow none of them. Two walks
/// over the same model answering "where is this" independently — and when they
/// disagreed nothing said so, because *having no CRS is legal*. A frame whose
/// nodes sat in a `ZIncreasingDownward` system was reported as having no
/// system, so [`Crs::to_projected`] never ran on it: the trajectory's depths
/// were flipped to elevations and its own log points were not. The well went
/// down and its logs went up, and every individual number looked reasonable.
///
/// So the rule is one place, and a representation that starts borrowing
/// geometry tomorrow costs **one string here** rather than a second walk that
/// has to be remembered. `crossaudit.rs` reads this same list to decide what to
/// confront, so adding the string also buys the check that would catch getting
/// it wrong.
///
/// # What the standard says, rather than what the corpus showed
///
/// The corpus is how this was found; the RESQML 2.2 Technical Reference Guide
/// is why it is right. Three statements settle it, and none of them is a
/// reading between lines:
///
/// - **§3.1** — *"`AbstractGeometry.LocalCrs` → `LocalEngineeringCompoundCrs`.
///   The base class for all geometric values, which is always associated with a
///   representation."* The system lives on the **geometry**, not on the
///   representation.
/// - **§12.17** — `WellboreTrajectoryRepresentation.Geometry 0..1 →
///   AbstractParametricLineGeometry`, which specialises `AbstractGeometry`. So
///   a trajectory that has a shape states its system there, and one that has
///   none states it nowhere — the two branches `crate::well` reads.
/// - **§12.13** — `WellboreFrameRepresentation` declares `NodeCount`, `NodeMd`,
///   `WitsmlLog` and the `Trajectory` association, and **no geometry and no
///   CRS of any kind**, under the *BUSINESS RULE: MD values and UOM must be
///   consistent with the trajectory representation*.
///
/// A frame's node positions are therefore not stored anywhere: they are the
/// trajectory's parametric line evaluated at the frame's own measured depths.
/// Inheriting the trajectory's system is not a heuristic that happens to fit
/// the files — it is the only reading the model allows. Checked against the
/// generated schema as a second source: neither the 2.0.1 nor the 2.2
/// `WellboreFrameRepresentation` has a single CRS-shaped field.
///
/// `SubRepresentation` is the same argument (§8.9): *"an ordered list of
/// indexable elements … of an existing representation"* — indices into someone
/// else's nodes, so someone else's system.
///
/// # Two limits, both deliberate
///
/// `Grid` is a **list** — a connection set may join several grids — and it is
/// honoured only when every grid it names agrees on the system. See the loop.
///
/// RESQML 2.2 puts `SupportingRepresentation` on the **patch** (§8.10: *"one
/// patch of a sub-representation is defined on one supporting
/// representation"*, and patches may name different ones), so a 2.2
/// sub-representation can draw from several supports. Since 02/10/2026 it
/// borrows by the same rule as `Grid` (`patch_supports_agree`): all the
/// patches' supports in one system, or nothing — never the first patch's,
/// which would be a guess wearing a number. RESQML 2.0.1 carries it on the
/// object and is covered by this list.
///
/// A `SeismicWellboreFrameRepresentation` never reaches here: §12.9 gives it
/// its own `LocalTime3dCrs`, which [`reference`] finds by name, so it takes the
/// direct path. That is what the standard asks for — its vertical axis is a
/// time and the trajectory's is a depth. (Nothing draws one today: the geometry
/// readers are keyed on `WellboreFrameRepresentation` alone.)
pub const BORROWED_GEOMETRY_FIELDS: &[&str] = &["Trajectory", "SupportingRepresentation", "Grid"];

/// The parent grid reference, under every name the two versions give it.
///
/// RESQML 2.0.1 calls it `ParentGrid` whichever kind of window it is. 2.2
/// renamed it once per kind. The strings are the whole of the version
/// difference, which is why they live here rather than in a `match` on version.
pub const PARENT_GRID_FIELDS: &[&str] = &[
    "ParentGrid",
    "ParentIjkGridRepresentation",
    "ParentColumnLayerGridRepresentation",
    "ParentGridRepresentation",
];

/// The CRS a local grid inherits from the grid it refines.
///
/// The hop count bounds two things at once: a legal chain of local grids —
/// RESQML allows a local grid of a local grid — and an illegal cycle, which
/// nothing in a file forbids two grids from forming by naming each other.
fn from_parent_window(object: &dyn Reflect, source: &dyn ObjectSource, hop: u8) -> Result<Option<Crs>> {
    if hop > 4 {
        return Ok(None);
    }
    let Some(FieldValue::Object(window)) = object.field("ParentWindow") else {
        return Ok(None);
    };
    let Some(named) = PARENT_GRID_FIELDS
        .iter()
        .find_map(|field| reference_to(window, field))
    else {
        return Ok(None);
    };
    let Some(parent) = source.object_of(named) else {
        return Err(Error::Missing(format!("the parent grid {}", uri_or_uuid(named))));
    };
    let parent = parent.as_reflect();
    match reference_in(parent) {
        Some(crs_named) => {
            let crs = source
                .object_of(crs_named)
                .ok_or_else(|| Error::Missing(format!("the CRS {}", uri_or_uuid(crs_named))))?;
            Ok(Some(read_with(crs.as_reflect(), source)?))
        }
        None => from_parent_window(parent, source, hop + 1),
    }
}

/// The objects a measured depth is counted from, under every name the releases
/// give them — the one list, which `energyml-geometry::well` reads too.
///
/// - `MdDatum` — RESQML 2.0.1 and the 2.2 development drops: a `Location` in a
///   whole local CRS;
/// - `ReferencePointInACrs` — eml 2.3, a projected and a vertical half named
///   separately (common TRG 2.3 §13.24 p. 370);
/// - `ReferencePointInALocalEngineeringCompoundCrs` — eml 2.3, local
///   coordinates of the compound its `Crs` names (§13.25 p. 371). The form
///   an external translation tool writes for a translated `obj_MdDatum`, and the form
///   chosen for a datum under a compound.
pub const DATUM_TYPES: &[&str] = &["ReferencePointInACrs", DATUM_IN_COMPOUND, "MdDatum"];

/// The reference point written in the local coordinates of a
/// `LocalEngineeringCompoundCrs` — common TRG 2.3 §13.25 p. 371.
pub const DATUM_IN_COMPOUND: &str = "ReferencePointInALocalEngineeringCompoundCrs";

/// The compound a `ReferencePointInALocalEngineeringCompoundCrs` is written
/// in, read whole — `Ok(None)` for any other object.
///
/// # Why its coordinates are placed through it
///
/// The guide types the association — *"From:
/// ReferencePointInALocalEngineeringCompoundCrs.Crs To:
/// LocalEngineeringCompoundCrs"* (common TRG 2.3 §13.25 p. 371) — so the
/// point's `HorizontalCoordinates` and `VerticalCoordinate` are **local**
/// coordinates of that compound, like any geometry that names it: counted
/// along its axes, from its origin, in its units. Where the point is on the
/// map is [`Crs::to_projected`] of them, and nothing else: read raw, a datum
/// under a compound with an origin or an azimuth sits where its surfaces are
/// not, with every number looking reasonable.
///
/// Anything but a `LocalEngineeringCompoundCrs` at the end of `Crs` is refused
/// by name: the XSD leaves the reference untyped, and a `ProjectedCrs` there
/// would say nothing of which way the vertical coordinate counts.
pub fn compound_of_datum(datum: &dyn Reflect, source: &dyn ObjectSource) -> Result<Option<Crs>> {
    if datum.type_info().type_key != DATUM_IN_COMPOUND {
        return Ok(None);
    }
    let at = format!(
        "{DATUM_IN_COMPOUND} {}",
        identity::uuid_of_reflect(datum).unwrap_or_default()
    );
    let Some(named) = reference_to(datum, "Crs") else {
        return Err(Error::malformed(at, "`Crs` names no coordinate system"));
    };
    let Some(system) = source.object_of(named) else {
        return Err(Error::Missing(format!(
            "the CRS {} of the reference point ({at})",
            uri_or_uuid(named)
        )));
    };
    let crs = read_with(system.as_reflect(), source)?;
    if crs.kind != "LocalEngineeringCompoundCrs" {
        return Err(Error::unsupported(
            format!("a reference point whose `Crs` names a `{}`", crs.kind),
            format!(
                "{at}: common TRG 2.3 §13.25 p. 371 gives `Crs` to a `LocalEngineeringCompoundCrs`, the only kind that says which way its vertical coordinate counts"
            ),
        ));
    }
    Ok(Some(crs))
}

/// The CRS a well is placed in, taken from its measured-depth datum.
///
/// A `ReferencePointInACrs` names a projected CRS for its horizontal
/// coordinates and a vertical CRS for its elevation. The geometry layer
/// evaluates such wells into **absolute projected coordinates, z-up** — so the
/// `Crs` built here carries no offset, no rotation, no unit scale: only the
/// EPSG identity that makes the points exportable and reprojectable.
///
/// A `ReferencePointInALocalEngineeringCompoundCrs` gets the same identity
/// view: the geometry layer places it through its compound
/// ([`compound_of_datum`]) and emits the same absolute coordinates, so the
/// compound's offsets must not reach the placement layer a second time.
fn from_well_datum(object: &dyn Reflect, source: &dyn ObjectSource, hop: u8) -> Result<Option<Crs>> {
    if hop > 1 {
        return Ok(None);
    }
    // A frame names its trajectory, and the trajectory carries the interval.
    if let Some(FieldValue::Object(trajectory)) = object.field("Trajectory")
        && let Some(resolved) = source.object_of(trajectory)
    {
        return from_well_datum(resolved.as_reflect(), source, hop + 1);
    }

    // The bounds are grouped in an `MdInterval` (2.2) or flat on the trajectory
    // (2.0.1 and the 2.2 development drops), and the datum is named wherever
    // they are. Falling back to the object itself is what lets one reader serve
    // both spellings.
    let interval = match object.field("MdInterval") {
        Some(FieldValue::Object(interval)) => interval,
        _ => object,
    };
    let Some(datum_named) = reference_to(interval, "Datum").or_else(|| reference_to(interval, "MdDatum"))
    else {
        return Ok(None);
    };
    let datum_uuid = identity::uuid_of_reflect(datum_named)
        .unwrap_or_default()
        .to_string();
    let Some(datum) = source.object_of(datum_named) else {
        return Err(Error::Missing(format!("the datum {}", uri_or_uuid(datum_named))));
    };
    let datum = datum.as_reflect();
    // `MdDatum` in 2.0.1 and the drops, two reference points in 2.2 — one
    // concept under three names.
    if !DATUM_TYPES.contains(&datum.type_info().type_key) {
        return Ok(None);
    }

    // **A point in a compound: the compound's identity, and none of its
    // placement.** Its offsets, azimuth and vertical origin are applied by the
    // geometry layer, which places the datum through them and emits absolute
    // projected coordinates, z-up and in metres; leaving them here would apply
    // them a second time. A time compound is refused rather than read: a
    // measured depth is a length, and nothing here converts one into a time.
    if let Some(compound) = compound_of_datum(datum, source)? {
        if compound.is_time() {
            return Err(Error::unsupported(
                "a measured-depth datum placed in a time CRS",
                format!(
                    "the reference point {datum_uuid} is written in {}, whose vertical axis is a time",
                    compound.uuid
                ),
            ));
        }
        return Ok(Some(Crs {
            uuid: datum_uuid,
            kind: DATUM_IN_COMPOUND,
            x_offset: 0.0,
            y_offset: 0.0,
            z_offset: 0.0,
            areal_rotation: 0.0,
            vertical_direction: VerticalDirection::Up,
            vertical_quantity: VerticalQuantity::Length,
            // The placed x and y are in the projected system's unit, which the
            // compound's horizontal axes state; the elevation has been
            // converted to metres to meet the measured depths.
            horizontal_uom: compound.horizontal_uom,
            vertical_uom: Some("m".to_string()),
            time_uom: None,
            projected: compound.projected,
            vertical: compound.vertical,
        }));
    }

    // **An `MdDatum` names a whole CRS, so read that rather than assembling
    // one.** Its `Location.LocalCrs` is a `LocalDepth3dCrs` carrying offsets,
    // EPSG code and vertical direction together; the 2.2 shape has no such
    // object and has to be built from a horizontal and a vertical half. Taking
    // the whole one matters: a package whose local CRS has non-zero offsets
    // would otherwise put its wells somewhere its surfaces are not, and every
    // individual number would still look reasonable.
    if let Some(FieldValue::Object(location)) = datum.field("Location")
        && let Some(local_named) = reference_to(location, "LocalCrs")
        && let Some(local) = source.object_of(local_named)
    {
        return read_with(local.as_reflect(), source).map(Some);
    }

    let projected = crs_object_kind(datum, "HorizontalCoordinates", source);
    let vertical: CrsKind = crs_object_kind(datum, "VerticalCrs", source);

    let out = Crs {
        uuid: datum_uuid,
        kind: "ReferencePointInACrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        // z-up by construction: the geometry layer emits well points as
        // elevations, converting the datum's own convention on the way.
        vertical_direction: VerticalDirection::Up,
        vertical_quantity: VerticalQuantity::Length,
        horizontal_uom: None,
        vertical_uom: None,
        time_uom: None,
        projected,
        vertical,
    };

    // if let Some(horizontal_coordinates) = object.field("HorizontalCoordinates") {
    //     out.x_offset = scalar_f64(&horizontal_coordinates, "Coordinate1").unwrap_or(0.0);
    //     out.y_offset = scalar_f64(&horizontal_coordinates, "Coordinate2").unwrap_or(0.0);
    // }

    Ok(Some(out))
}

/// The EPSG identity of the CRS a datum field points at.
///
/// The field is either the DOR itself (`VerticalCrs`) or a coordinates block
/// holding one (`HorizontalCoordinates.Crs`). The referenced object — an
/// eml 2.3 `ProjectedCrs` or `VerticalCrs` — carries its code inside its
/// `AbstractProjectedCrs`/`AbstractVerticalCrs` choice, which is what
/// [`kind_of`] already knows how to read; a shallow walk finds the right
/// field without hard-coding its name per version.
fn crs_object_kind(datum: &dyn Reflect, field: &str, source: &dyn ObjectSource) -> CrsKind {
    let Some(FieldValue::Object(holder)) = datum.field(field) else {
        return CrsKind::Absent;
    };
    // Either the holder is the DOR, or it wraps one under `Crs`.
    let named = if identity::uuid_of_reflect(holder).is_some() {
        Some(holder)
    } else {
        reference_to(holder, "Crs")
    };
    let Some(named) = named else {
        return CrsKind::Absent;
    };
    let Some(crs) = source.object_of(named) else {
        return CrsKind::Unknown;
    };
    let crs = crs.as_reflect();
    for field in crs.type_info().fields {
        let kind = kind_of(crs, field.name);
        if !matches!(kind, CrsKind::Absent) {
            return kind;
        }
    }
    CrsKind::Unknown
}

/// Read a self-contained CRS object, following nothing.
///
/// Enough for RESQML 2.0.1. An eml 2.3 compound CRS keeps two thirds of itself
/// in other objects, so that one needs [`read_with`].
pub fn read(crs: &dyn Reflect) -> Result<Crs> {
    read_with(crs, &crate::source::NoObjects)
}

/// Read a CRS, following the objects it references.
///
/// # The rotation: one sense in both releases, and not this view's
///
/// This view turns anticlockwise from the first projected axis, and neither
/// release does. RESQML 2.0.1: *"ArealRotation — The rotation of the local Y
/// axis relative to the projected Y axis. A positive value indicates a
/// clockwise rotation from the projected Y axis"* (RESQML TRG 2.0.1 §2.1
/// p. 10). eml 2.3's `LocalEngineering2dCrs.Azimuth` is not documented at all —
/// neither the schema nor common TRG 2.3 §12.19 p. 321 says which way it turns
/// — so it is read as the family reads an azimuth: from the projected north,
/// *"with a positive value indicating a clockwise rotation as seen from above"*
/// (RESQML TRG 2.0.1 §7.5.7 p. 140, the survey azimuths), which is also the
/// reading under which an external translation tool carries `ArealRotation` into
/// `Azimuth` unchanged. Writing the local y axis both ways —
/// `(−sin θ, cos θ)` here against `(sin A, cos A)` there — gives `θ = −A`, for
/// both.
///
/// Until 24/09/2026 the 2.0.1 angle was taken as it stands, anticlockwise: a
/// rotated 2.0.1 model and its own translation turned in opposite directions
/// under this one reader. Nothing in the corpus is rotated, so nothing showed.
///
/// `AzimuthReference` says what north means (common TRG 2.3 §10.14 p. 226).
/// Anything but grid north needs a convergence angle this crate does not have,
/// so a non-zero azimuth measured from something else is **refused** rather
/// than placed wrongly. A zero azimuth is the same in every reference, and zero
/// is what every file in the corpus uses — so these conversions are held by
/// unit tests built from the citations, never by a file.
///
/// # The vertical: the axis governs, the datum places
///
/// On an eml 2.3 compound, `VerticalAxis` sets the direction, the unit and the
/// time flag of the local z; the referenced `VerticalCrs` sets the datum's EPSG
/// identity, and its own direction and unit are only used to bring
/// `OriginVerticalCoordinate` — written *"in the base vertical CRS"* — into
/// the axis's sense and unit (`origin_along_axis`). See the module doc.
pub fn read_with(crs: &dyn Reflect, source: &dyn ObjectSource) -> Result<Crs> {
    let mut out = Crs {
        uuid: identity::uuid_of_reflect(crs).unwrap_or_default().to_string(),
        kind: crs.type_info().name,
        x_offset: scalar_f64(crs, "XOffset").unwrap_or(0.0),
        y_offset: scalar_f64(crs, "YOffset").unwrap_or(0.0),
        // RESQML 2.0.1's `ZOffset`, along the local axis — its schema makes that
        // axis count the way the vertical CRS does. eml 2.3's origin is not
        // along the axis, and is converted once both halves are read, below.
        z_offset: scalar_f64(crs, "ZOffset").unwrap_or(0.0),
        areal_rotation: areal_rotation(crs)?,
        vertical_direction: if flag(crs, "ZIncreasingDownward") {
            VerticalDirection::Down
        } else {
            VerticalDirection::Up
        },
        // 2.0.1 says it in the type name; 2.3 says it with a flag, read below.
        vertical_quantity: if crs.type_info().name.contains("Time") {
            VerticalQuantity::Time
        } else {
            VerticalQuantity::Length
        },
        horizontal_uom: measure::enum_value(crs, "ProjectedUom"),
        vertical_uom: measure::enum_value(crs, "VerticalUom"),
        time_uom: measure::enum_value(crs, "TimeUom"),
        projected: kind_of(crs, "ProjectedCrs"),
        vertical: kind_of(crs, "VerticalCrs"),
    };

    // The object may *be* a vertical system rather than merely name one — a
    // standalone eml 2.3 `VerticalCrs` is a top-level object in its own right.
    if let Some(direction) = direction_of(crs) {
        out.vertical_direction = direction;
    }
    if out.vertical == CrsKind::Absent {
        let itself = kind_of(crs, "AbstractVerticalCrs");
        if itself != CrsKind::Absent {
            out.vertical = itself;
        }
    }
    // And the same for the projected half, which had no such line until
    // 12/08 — so a geometry whose `LocalCrs` names an eml 2.3 `ProjectedCrs`
    // **directly**, rather than the compound that usually wraps it, came back
    // saying "no EPSG code" over a file whose next two lines read
    // `<AbstractProjectedCrs xsi:type="ProjectedEpsgCrs"><EpsgCode>3949`.
    //
    // Nothing in the corpus pointed at one, and the schema does not forbid it:
    // `AbstractGeometry.LocalCrs` is a `DataObjectReference`, which declares no
    // target type at all. A `ProjectedCrs` read on its own is a perfectly
    // ordinary answer — origin at (0, 0), no rotation, one EPSG code — and
    // refusing it lost the only thing the file was unambiguous about.
    if out.projected == CrsKind::Absent {
        let itself = kind_of(crs, "AbstractProjectedCrs");
        if itself != CrsKind::Absent {
            out.projected = itself;
        }
    }
    // And a **`Geographic2dCrs`**, added 08/09/2026 for the same reason one
    // line up, in the same place, from the same evidence: a real package whose
    // next two lines read `<AbstractGeographic2dCrs xsi:type="GeodeticEpsgCrs">
    // <EpsgCode>4326`, and which came back saying *no EPSG code*.
    //
    // `energyml-build` writes them: a GeoJSON is WGS 84 by definition, and the
    // decision of 07/09 is to keep the degrees rather than transform them — a
    // transformation being where coordinates quietly move. EML 2.3 offers no
    // top-level geographic compound, so the geometry names the
    // `Geographic2dCrs` directly, exactly as an eml 2.3 geometry may name a
    // `ProjectedCrs` directly. Before this, `energyml export --format geojson`
    // refused those packages — *the CRS they name gives no EPSG code* — over a
    // file that was unambiguous.
    //
    // **The hazard, and it is the reason `horizontal_uom` is left alone.** A
    // geographic system's horizontal unit is the **degree**, not the metre, and
    // a `Geographic2dCrs` carries no `HorizontalAxes` to say so — so the unit
    // stays `None`, which reads as *unstated* rather than as *metres*. A caller
    // that defaulted an absent unit to metres would scale a Paris-basin model
    // by 111 320; one that reads `None` as *unknown* is right. This is why the
    // field is not filled with a guess.
    if out.projected == CrsKind::Absent {
        let itself = kind_of(crs, "AbstractGeographic2dCrs");
        if itself != CrsKind::Absent {
            out.projected = itself;
        }
    }

    // eml 2.3 keeps the vertical axis on the compound itself, and **it** says
    // how the stored z counts: direction, unit, length or time (common TRG 2.3
    // §12.34 p. 338). Nothing read below overwrites what it states.
    let axis = field_object(crs, "VerticalAxis");
    if let Some(axis) = axis {
        read_axis(&mut axis_target(&mut out, true), axis);
        // When that axis is a **time** axis, its unit is the time unit, and it
        // moves to `time_uom`: `vertical_uom` then holds the length of the
        // datum, read from the `VerticalCrs` below. eml 2.3 keeps the same two
        // facts 2.0.1 keeps in `TimeUom` and `VerticalUom`, in two objects
        // rather than two fields, and the reading has to end with both in the
        // same places whichever release it came from.
        if out.vertical_quantity == VerticalQuantity::Time {
            out.time_uom = out.vertical_uom.take();
        }
    }

    // eml 2.3: the horizontal half is a whole other object.
    let owned = linked(crs, "LocalEngineering2dCrs", source);
    if let Some(plane) = owned
        .as_ref()
        .map(|object| object.as_reflect())
        .or_else(|| field_object(crs, "LocalEngineering2dCrs"))
    {
        // The origin, in the projected system: the 2.3 schema documents
        // neither element, and they are read as the 2.0.1 offsets they
        // replaced — *"The X location of the origin of the local areal axes
        // relative to the projected CRS origin. […] The value MUST represent
        // the first axis of the coordinate system. The unit of measure is
        // defined by the unit of measure for the projected 2D CRS"* (RESQML TRG
        // 2.0.1 §2.1 p. 11), which is also how an external translation tool carries
        // them. One unit is assumed for the offsets and the local axes
        // (`HorizontalAxes.Uom`), as 2.0.1 required of its `ProjectedUom`; the
        // corpus never states two.
        if let Some(x) = scalar_f64(plane, "OriginProjectedCoordinate1") {
            out.x_offset = x;
        }
        if let Some(y) = scalar_f64(plane, "OriginProjectedCoordinate2") {
            out.y_offset = y;
        }
        if plane.field("Azimuth").is_some() {
            out.areal_rotation = azimuth_as_rotation(plane)?;
        }
        // `HorizontalAxes.IsTime` is about x and y, and is not read into the
        // vertical quantity — the `VerticalAxis` alone says whether z is a
        // time. It did until 24/09, so a compound whose horizontal axes said
        // `true` read as a time system whatever its vertical axis said. A
        // horizontal axis in time would carry a time `Uom`, which
        // `to_metres` refuses by name.
        if let Some(axes) = field_object(plane, "HorizontalAxes")
            && let Some(unit) = measure::enum_value(axes, "Uom")
        {
            out.horizontal_uom = Some(unit);
        }
        // The EPSG code sits one level further down, inside `OriginProjectedCrs`.
        if let Some(origin) = field_object(plane, "OriginProjectedCrs") {
            let found = kind_of(origin, "AbstractProjectedCrs");
            if found != CrsKind::Absent {
                out.projected = found;
            }
        }
    }

    // eml 2.3: the vertical half, a referenced top-level object — the datum.
    let owned = linked(crs, "VerticalCrs", source);
    let (mut datum_direction, mut datum_uom): (Option<VerticalDirection>, Option<String>) = (None, None);
    if let Some(vertical) = owned.as_ref().map(|object| object.as_reflect()) {
        let found = kind_of(vertical, "AbstractVerticalCrs");
        if found != CrsKind::Absent {
            out.vertical = found;
        }
        datum_direction = direction_of(vertical);
        datum_uom = measure::unit_of(vertical);
        match axis {
            // No axis above it: the object names a vertical system without
            // being a compound — a `ReferencePointInACrs` — and that system is
            // then the only statement of which way its z counts.
            None => {
                if let Some(direction) = datum_direction {
                    out.vertical_direction = direction;
                }
                if let Some(unit) = &datum_uom {
                    out.vertical_uom = Some(unit.clone());
                }
            }
            // A time axis: the datum's unit is the length `vertical_uom` holds,
            // as 2.0.1's `VerticalUom` does beside its `TimeUom`.
            Some(_) if out.vertical_quantity == VerticalQuantity::Time => {
                if let Some(unit) = &datum_uom {
                    out.vertical_uom = Some(unit.clone());
                }
            }
            // A length axis keeps its own direction and unit: until 24/09 the
            // datum's overwrote both, and a compound counting down from a datum
            // counting up was read counting up.
            Some(_) => {}
        }
    }

    // eml 2.3's origin, now that both the axis and the datum are known.
    if let Some(origin) = scalar_f64(crs, "OriginVerticalCoordinate") {
        out.z_offset = origin_along_axis(origin, &out, datum_direction, datum_uom.as_deref())?;
    }
    Ok(out)
}

/// `OriginVerticalCoordinate`, as the offset this view keeps: along the
/// **local** axis, counted the way that axis counts, in its unit.
///
/// The schema writes it elsewhere: *"Vertical coordinate of the origin of the
/// local engineering CRS in the base vertical CRS (consequently in the uom of
/// the base vertical CRS)"* (CRS.xsd l. 517–520; common TRG 2.3 §12.20 p. 322).
/// So the number is negated when the axis and the datum count in opposite
/// directions, and converted from the datum's unit to the axis's. An origin 35
/// m above an NGF datum that counts up, under an axis that counts depths, is
/// −35 along that axis.
///
/// Two limits, both said rather than guessed:
///
/// - **a datum that cannot be read** leaves the sense of a non-zero origin
///   unknown, and the reading is refused: taking the axis's sense instead is
///   exactly the error this function exists to stop. A zero origin needs no
///   sense and passes;
/// - **a time axis** keeps the origin as written. It is a length in the datum
///   and the axis counts time, and nothing here holds the velocity that joins
///   them — the disagreement a translation tool names. The number
///   stays in `vertical_uom`, which on a time system is the datum's length;
///   [`Crs::to_metres`] refuses a time system anyway, and [`Crs::is_time`]
///   tells a caller not to mix it with depths.
fn origin_along_axis(
    origin: f64,
    axis: &Crs,
    datum_direction: Option<VerticalDirection>,
    datum_uom: Option<&str>,
) -> Result<f64> {
    if origin == 0.0 {
        return Ok(0.0);
    }
    let Some(datum_direction) = datum_direction else {
        return Err(Error::unsupported(
            "a vertical origin whose datum does not say which way it counts",
            format!(
                "{} {}: `OriginVerticalCoordinate` {origin} is written in the base vertical CRS, and that `VerticalCrs` is not in the store or states no `Direction`",
                axis.kind, axis.uuid
            ),
        ));
    };
    let along = if datum_direction == axis.vertical_direction {
        origin
    } else {
        -origin
    };
    if axis.is_time() {
        return Ok(along);
    }
    match (datum_uom, axis.vertical_uom.as_deref()) {
        (Some(from), Some(to)) => units::convert(along, from, to),
        // One side states no unit: taken at its word, as everywhere here.
        _ => Ok(along),
    }
}

/// The uuid of the CRS an object names, at any depth.
///
/// A representation names it on itself or on a patch geometry; the first one
/// found is the object's. An adapter for callers that hold a uuid;
/// [`reference_uri`] is the full identity, version included.
pub fn reference(object: &dyn Reflect) -> Option<String> {
    reference_in(object).and_then(|named| identity::uuid_of_reflect(named).map(str::to_string))
}

/// The **uri** of the CRS an object names, at any depth — the version the
/// reference declares included, which is what [`ObjectSource::object_at`] reads.
pub fn reference_uri(object: &dyn Reflect) -> Option<String> {
    reference_in(object).and_then(identity::uri_of_reflect)
}

/// The reference itself: what [`ObjectSource::object_of`] follows.
fn reference_in(object: &dyn Reflect) -> Option<&dyn Reflect> {
    for field in ["LocalCrs", "Crs", "LocalDepth3dCrs", "LocalTime3dCrs"] {
        if let Some(named) = reference_to(object, field) {
            return Some(named);
        }
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => {
                if let Some(named) = reference_in(child) {
                    return Some(named);
                }
            }
            Some(FieldValue::List(items)) => {
                for item in items {
                    if let Some(child) = item.as_object()
                        && let Some(named) = reference_in(child)
                    {
                        return Some(named);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------

/// Borrow the parts of a `Crs` an axis element can set.
struct AxisTarget<'a> {
    direction: &'a mut VerticalDirection,
    quantity: &'a mut VerticalQuantity,
    uom: &'a mut Option<String>,
}

fn axis_target(crs: &mut Crs, vertical: bool) -> AxisTarget<'_> {
    debug_assert!(vertical, "only the vertical axis is read today");
    AxisTarget {
        direction: &mut crs.vertical_direction,
        quantity: &mut crs.vertical_quantity,
        uom: &mut crs.vertical_uom,
    }
}

/// `VerticalAxis` / `HorizontalAxes`: direction, unit, and length-or-time.
fn read_axis(target: &mut AxisTarget<'_>, axis: &dyn Reflect) {
    if let Some(direction) = direction_of(axis) {
        *target.direction = direction;
    }
    if let Some(unit) = measure::enum_value(axis, "Uom") {
        *target.uom = Some(unit);
    }
    if flag(axis, "IsTime") {
        *target.quantity = VerticalQuantity::Time;
    }
}

/// `Direction` as `up` / `down`, eml 2.3's way of saying `ZIncreasingDownward`.
fn direction_of(object: &dyn Reflect) -> Option<VerticalDirection> {
    let value = measure::enum_value(object, "Direction")?;
    if value.eq_ignore_ascii_case("down") {
        Some(VerticalDirection::Down)
    } else if value.eq_ignore_ascii_case("up") {
        Some(VerticalDirection::Up)
    } else {
        None
    }
}

/// A field that is a reference, resolved through the store.
///
/// `None` when absent, uuid-less, or naming something the store lacks — the
/// caller falls back to reading it inline.
fn linked(
    owner: &dyn Reflect,
    field: &str,
    source: &dyn ObjectSource,
) -> Option<Box<dyn energyml_core::registry::DynObject>> {
    let FieldValue::Object(child) = owner.field(field)? else {
        return None;
    };
    let uuid = identity::uuid_of_reflect(child)?;
    if uuid.is_empty() {
        return None;
    }
    source.object_of(child)
}

/// RESQML 2.0.1's `ArealRotation`, as the anticlockwise-from-east rotation
/// used here: its negation, the angle being clockwise from the projected Y
/// axis (RESQML TRG 2.0.1 §2.1 p. 10) — see [`read_with`].
fn areal_rotation(crs: &dyn Reflect) -> Result<f64> {
    let rotation = angle(crs, "ArealRotation")?;
    // Zero stays zero rather than becoming −0, which prints as a rotation.
    Ok(if rotation == 0.0 { 0.0 } else { -rotation })
}

/// eml 2.3's `Azimuth`, as the anticlockwise-from-east rotation used here.
///
/// Its unit is the measure's own `uom` (`PlaneAngleMeasureExt`, CRS.xsd
/// l. 499), converted by [`angle`]; one outside the table is refused there.
fn azimuth_as_rotation(plane: &dyn Reflect) -> Result<f64> {
    let azimuth = angle(plane, "Azimuth")?;
    if azimuth == 0.0 {
        return Ok(0.0);
    }
    let reference = measure::enum_value(plane, "AzimuthReference").unwrap_or_default();
    if !reference.eq_ignore_ascii_case("grid north") {
        return Err(Error::unsupported(
            "an azimuth measured from something other than grid north",
            format!("LocalEngineering2dCrs.AzimuthReference = `{reference}`"),
        ));
    }
    Ok(-azimuth)
}

/// Which branch a projected or vertical component took.
///
/// The choice element is named after the branch in every version —
/// `ProjectedEpsgCrs`, `VerticalUnknownCrs` — except eml 2.0, which spells the
/// EPSG one `ProjectedCrsEpsgCode` / `VerticalCrsEpsgCode`. Both are matched.
fn kind_of(crs: &dyn Reflect, field: &str) -> CrsKind {
    let Some(FieldValue::Object(child)) = crs.field(field) else {
        return CrsKind::Absent;
    };
    let name = child.type_info().name;
    if name.contains("Epsg") {
        for candidate in ["EpsgCode", "Epsg", "Code"] {
            if let Some(code) = scalar_i64(child, candidate)
                && code > 0
            {
                return CrsKind::Epsg(code as u64);
            }
        }
        return CrsKind::Unknown;
    }
    if name.contains("Wkt") {
        return CrsKind::Wkt(first_string(child).unwrap_or_default());
    }
    if name.contains("LocalAuthority") {
        return CrsKind::LocalAuthority(first_string(child).unwrap_or_default());
    }
    if name.contains("Gml") {
        return CrsKind::Gml;
    }
    if name.contains("Unknown") {
        return CrsKind::Unknown;
    }
    CrsKind::Absent
}

fn first_string(object: &dyn Reflect) -> Option<String> {
    for field in object.type_info().fields {
        if let Some(FieldValue::Scalar(scalar)) = object.field(field.name)
            && let Some(text) = scalar.as_str()
        {
            return Some(text.to_string());
        }
    }
    None
}

/// A measure, in the base unit of its family.
fn angle(owner: &dyn Reflect, field: &str) -> Result<f64> {
    let Some(FieldValue::Object(element)) = owner.field(field) else {
        return Ok(0.0);
    };
    let Some(found) = measure::read(element) else {
        return Ok(0.0);
    };
    match found.uom {
        // The schema's own default is radians.
        None => Ok(found.value),
        Some(unit) if found.value == 0.0 && units::Dimension::of(&unit).is_none() => Ok(0.0),
        Some(unit) => units::radians(found.value, &unit),
    }
}

fn flag(object: &dyn Reflect, field: &str) -> bool {
    matches!(
        object.field(field),
        Some(FieldValue::Scalar(energyml_core::value::Scalar::Bool(true)))
    )
}

fn scalar_f64(object: &dyn Reflect, field: &str) -> Option<f64> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar.as_f64(),
        _ => None,
    }
}

fn scalar_i64(object: &dyn Reflect, field: &str) -> Option<i64> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar.as_f64().map(|v| v as i64),
        _ => None,
    }
}

/// The reference a field holds, when it names an object at all.
fn reference_to<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    let FieldValue::Object(reference) = object.field(field)? else {
        return None;
    };
    identity::uuid_of_reflect(reference)
        .filter(|uuid| !uuid.is_empty())
        .map(|_| reference)
}

/// How a message names a reference: its uri, or its uuid when it has no uri.
fn uri_or_uuid(reference: &dyn Reflect) -> String {
    identity::uri_of_reflect(reference)
        .or_else(|| identity::uuid_of_reflect(reference).map(str::to_string))
        .unwrap_or_default()
}

fn field_object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crs(rotation: f64, direction: VerticalDirection) -> Crs {
        Crs {
            uuid: "test".into(),
            kind: "LocalDepth3dCrs",
            x_offset: 0.0,
            y_offset: 0.0,
            z_offset: 0.0,
            areal_rotation: rotation,
            vertical_direction: direction,
            vertical_quantity: VerticalQuantity::Length,
            horizontal_uom: None,
            vertical_uom: None,
            time_uom: None,
            projected: CrsKind::Absent,
            vertical: CrsKind::Absent,
        }
    }

    #[test]
    fn a_quarter_turn_swaps_the_axes() {
        let placed = crs(std::f64::consts::FRAC_PI_2, VerticalDirection::Up).to_projected([1.0, 0.0, 0.0]);
        assert!((placed[0]).abs() < 1e-12);
        assert!((placed[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_depth_becomes_an_elevation() {
        let placed = crs(0.0, VerticalDirection::Down).to_projected([0.0, 0.0, 1500.0]);
        assert_eq!(placed[2], -1500.0);
    }

    #[test]
    fn feet_are_not_metres() {
        // The bug this method exists to prevent: a file in feet read as metres
        // is off by a factor of 3.28, with every coordinate still plausible.
        let mut in_feet = crs(0.0, VerticalDirection::Up);
        in_feet.horizontal_uom = Some("ft".into());
        in_feet.vertical_uom = Some("ft".into());
        let placed = in_feet.to_metres([1000.0, 0.0, 0.0]).unwrap();
        assert!((placed[0] - 304.8).abs() < 1e-9);
        assert!(!in_feet.is_metric());
    }

    #[test]
    fn a_stated_unit_outside_the_table_is_refused() {
        let mut exotic = crs(0.0, VerticalDirection::Up);
        exotic.horizontal_uom = Some("exoticMeter".into());
        assert!(exotic.to_metres([1.0, 2.0, 3.0]).is_err());
    }

    #[test]
    fn a_silent_crs_is_left_alone() {
        // No unit stated is the schema's own default, not a guess.
        let quiet = crs(0.0, VerticalDirection::Up);
        assert_eq!(quiet.to_metres([1.0, 2.0, 3.0]).unwrap(), [1.0, 2.0, 3.0]);
        assert!(quiet.is_metric());
    }

    #[test]
    fn a_time_crs_refuses_to_become_metres() {
        let mut time = crs(0.0, VerticalDirection::Down);
        time.vertical_quantity = VerticalQuantity::Time;
        assert!(time.to_metres([0.0, 0.0, 1.0]).is_err());
    }

    /// An `eml23.Geographic2dCrs` **alone** returns its EPSG code.
    ///
    /// The case `energyml-build` writes: a GeoJSON is WGS 84 by definition, the
    /// decision of 07/09/2026 is to keep the degrees rather than transform them,
    /// and EML 2.3 offers no top-level geographic compound to wrap the pair — so
    /// the geometry names the `Geographic2dCrs` directly.
    ///
    /// Measured before the fix, on a package this workspace had just written:
    /// `energyml geometry` said `[Absent, unstated]` and
    /// `energyml export --format geojson` refused both representations —
    /// *the CRS they name gives no EPSG code* — over a file whose next two lines
    /// read `EpsgCode 4326`. Afterwards the same package exports 1 291 positions
    /// that match the source's, place for place.
    #[test]
    fn a_standalone_geographic_crs_states_its_own_epsg_code() {
        use energyml_eml_2_3 as eml23;

        let crs = eml23::Geographic2dCrs {
            uuid: "1f2f49a4-ed61-52e5-99e3-d4b3352cd04a".into(),
            abstract_geographic2d_crs: Some(eml23::AbstractGeographic2dCrs::GeodeticEpsgCrs(
                eml23::GeodeticEpsgCrs { epsg_code: 4326 },
            )),
            ..Default::default()
        };
        let read = read(&crs).expect("a geographic system reads on its own");
        assert_eq!(read.projected.epsg(), Some(4326));
        // **And the unit stays unstated.** A geographic system is in degrees,
        // a `Geographic2dCrs` carries no `HorizontalAxes` to say so, and a
        // caller that read an absent unit as metres would scale a Paris-basin
        // model by 111 320. `None` means unknown, and that is the true answer.
        assert_eq!(read.horizontal_uom, None);
    }

    /// An `eml23.ProjectedCrs` **alone** returns its EPSG code.
    ///
    /// The case of the BRGM package `result.epc`: the geometry's `LocalCrs`
    /// names the `ProjectedCrs` directly, not the
    /// `LocalEngineeringCompoundCrs` that usually wraps it. Nothing forbids it
    /// (`AbstractGeometry.LocalCrs` is a `DataObjectReference`, which declares
    /// no target type) and yet the reading returned "no EPSG code" on a file
    /// whose next two lines said `EpsgCode 3949`. The vertical half had always
    /// read on its own; the projected half had not, and no package of the
    /// corpus pointed at one.
    #[test]
    fn a_standalone_projected_crs_states_its_own_epsg_code() {
        use energyml_eml_2_3 as eml23;

        let crs = eml23::ProjectedCrs {
            uuid: "3a676705-07e4-46c3-ae76-8281a42c4607".into(),
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(
                eml23::ProjectedEpsgCrs { epsg_code: 3949 },
            )),
            ..Default::default()
        };
        let read = read(&crs).expect("a projected system reads on its own");
        assert_eq!(read.projected.epsg(), Some(3949));
        // What it does not say stays absent: a projected system carries no
        // vertical half, and inventing one would place the model.
        assert_eq!(read.vertical, CrsKind::Absent);
        assert_eq!(read.x_offset, 0.0);
        assert_eq!(read.y_offset, 0.0);
    }

    /// And a declaration naming no authority stays absent.
    ///
    /// The counterpart of the test above: without it, the reading just added
    /// could return anything that is not `Absent` and nobody would see it.
    #[test]
    fn a_projected_crs_that_names_nothing_stays_absent() {
        use energyml_eml_2_3 as eml23;

        let unknown = eml23::ProjectedCrs {
            uuid: "00000000-0000-0000-0000-0000000000ff".into(),
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedUnknownCrs(
                eml23::ProjectedUnknownCrs::default(),
            )),
            ..Default::default()
        };
        assert_eq!(read(&unknown).expect("se lit").projected, CrsKind::Unknown);

        let empty = eml23::ProjectedCrs::default();
        assert_eq!(read(&empty).expect("se lit").projected, CrsKind::Absent);
    }
}

#[cfg(test)]
mod inverse_tests {
    use super::*;

    #[test]
    fn from_projected_undoes_to_projected_exactly() {
        // The identity that stops a double placement: a promoted artefact
        // stores `from_projected(screen)`, and the read back applying
        // `to_projected` draws in the same place. All three traps at once -
        // offsets, rotation, downward axis - because the corpus has null
        // origins and would bite on none of them.
        let crs = Crs {
            uuid: "test".into(),
            kind: "LocalDepth3dCrs",
            x_offset: 700_000.0,
            y_offset: 6_600_000.0,
            z_offset: 120.0,
            areal_rotation: 0.3,
            vertical_direction: VerticalDirection::Down,
            vertical_quantity: VerticalQuantity::Length,
            horizontal_uom: None,
            vertical_uom: None,
            time_uom: None,
            projected: CrsKind::Absent,
            vertical: CrsKind::Absent,
        };
        for local in [[0.0, 0.0, 0.0], [12.5, -7.25, 33.0], [-100.0, 250.0, -8.5]] {
            let placed = crs.to_projected(local);
            let back = crs.from_projected(placed);
            for axis in 0..3 {
                assert!(
                    (back[axis] - local[axis]).abs() < 1e-9,
                    "axis {axis}: {local:?} → {placed:?} → {back:?}"
                );
            }
        }
    }
}
