//! The coordinate reference system a GeoJSON's numbers are already in.
//!
//! # Degrees, or metres — and why the choice had to exist
//!
//! Keeping the degrees is lossless and, for the 3D view, unusable. Measured on
//! a SIGEOL import: the horizontal extent is 0.124 **degree**
//! and the vertical 41.8 **metres**, so a viewer plotting the three ordinates in
//! one space draws a needle 335 times too tall. Physically that cloud is
//! ~9 km × 3.5 km × 42 m — almost flat.
//!
//! Nothing in the file can fix that on the reader's side: `Point3d` is three
//! bare doubles, a `Geographic2dCrs` carries no `HorizontalAxes` to say
//! *degrees*, and the conversion depends on the latitude. So **a coherent scene
//! needs projected metres**, and the caller says so with a target EPSG.
//!
//! Both remain: degrees for a package that must not move a coordinate,
//! projection for one that must be looked at. Neither is a default hidden in a
//! signature — [`Target`] makes the caller name it.
//!
//! # No *silent* reprojection, and that is still the design
//!
//! RFC 7946 makes a GeoJSON WGS 84 by definition, so the EPSG code is **known**
//! — 4326 — without anybody declaring it. The decision is to
//! write the data as it stands in that system and create the CRS objects the
//! package lacks, rather than transform the numbers into the package's own.
//!
//! It is the better trade by some distance. A transformation is where
//! coordinates quietly move: a datum shift applied twice, or with the wrong
//! seven parameters, produces a file that opens everywhere and is a few hundred
//! metres out. Writing degrees changes nothing, so nothing can be lost — and a
//! consumer that wants metres reprojects knowing what it started from.
//!
//! # What EML 2.3 lets us say, and what it does not
//!
//! `Geographic3dCrs` looks like the obvious answer and **cannot carry an EPSG
//! code at all**: its definition in `CRS.xsd` is `<xs:extension
//! base="eml:AbstractObject"><xs:sequence/></xs:extension>` — an object with no
//! content. Checked in the schema rather than assumed from the generated code,
//! because an empty generated struct is exactly what a generator bug looks like.
//! It is not one; the type is empty in the standard.
//!
//! So the halves are written as the schema offers them as *parts*:
//!
//! - a **`ProjectedCrs`** holding a `ProjectedEpsgCrs` — metres — or a
//!   **`Geographic2dCrs`** holding a `GeodeticEpsgCrs` — degrees, the one that
//!   carries 4326;
//! - a **`VerticalCrs`** — the elevation, its direction and its unit;
//! - and, over metres, the **`LocalEngineering2dCrs`** and the
//!   **`LocalEngineeringCompoundCrs`** that join them.
//!
//! # A geometry names a compound — and degrees are the one named exception
//!
//! RESQML's own guide types the reference the XSD leaves open:
//! *« From: AbstractGeometry.LocalCrs — To: LocalEngineeringCompoundCrs »*
//! (RESQML TRG v2.2, §3.1, p. 22; and §7.1, p. 222, for a property's
//! `LocalCrs`). This module used to write the two halves and point
//! every geometry at the **horizontal** one, which left the `VerticalCrs`
//! referenced by nothing — the one statement there is about the third ordinate
//! hanging loose in the package. It now
//! writes the compound, and [`Built::reference`] names it.
//!
//! **The compound written by default is the identity**, so that nothing an
//! operation writes moves: origin at 0/0 of the projected system, azimuth 0,
//! a vertical axis counting the way the `VerticalCrs` does, in metres, not a
//! time, with its origin at 0 of the vertical system. A number written under
//! it *is* a Lambert-93 easting, a Lambert-93 northing and an NGF elevation —
//! exactly what it was under the `ProjectedCrs` before. The `crs`
//! operation is the one that asks for an origin, an azimuth,
//! a depth or a time axis ([`Local`]).
//!
//! **Degrees have no compound**, and that is the schema's doing: a
//! `LocalEngineering2dCrs` requires an `OriginProjectedCrs` whose type is
//! `eml:ProjectedCrs` (`CRS.xsd` l. 504), and `GeographicCompoundCrs` is not a
//! top-level data object — `CRS.xsd` declares an `<xs:element …
//! substitutionGroup="eml:AbstractDataObject">` for `Geographic2dCrs`,
//! `VerticalCrs`, `ProjectedCrs`, `LocalEngineering2dCrs` and
//! `LocalEngineeringCompoundCrs` (l. 456, 669, 538, 494, 509), and for no
//! other. So a geometry in degrees keeps naming its `Geographic2dCrs`, and the
//! `VerticalCrs` beside it is still referenced by nothing. The decision is
//! **an exception, named** — every plan that writes one says so
//! in its notes (`frame-degrees`), rather than a refusal that would break
//! `sigeol-units`, which keeps degrees by default.
//!
//! # A compound on an unknown vertical is still a compound
//!
//! When nobody states the vertical datum, the `VerticalCrs` carries a
//! `VerticalUnknownCrs` — and it is still a `VerticalCrs` whose `Direction`
//! and `uom` are written: *"In this case, the uom and Direction need to be
//! provided on the VerticalCrs class"* (`CRS.xsd`, `VerticalUnknownCrs`,
//! l. 705). So the compound's `VerticalAxis` has something to agree with, and
//! the compound is written the same. What is unknown is the **datum**, and it
//! is said where it lives — in the `VerticalCrs` — not by withholding the frame
//! the geometry needs.
//!
//! Found by writing a package and reading it back (`tests/package.rs`); the
//! unit tests over objects in memory could not see it, because nothing in
//! memory knows what may be a part.
//!
//! # The vertical datum is the one thing nobody states
//!
//! RFC 7946 says the third ordinate is height above the WGS 84 ellipsoid. The
//! BRGM exports carry ground altitude at a borehole head, which in France is
//! NGF-IGN69 and not ellipsoidal — the two differ by some forty metres here.
//! **The file does not say which**, and the difference is exactly the kind that
//! never surfaces: both are plausible numbers of metres.
//!
//! So the caller names it, and when it does not, this writes a
//! **`VerticalUnknownCrs`** — a type the standard provides for saying precisely
//! this. Not a guess at 5714, not a silent ellipsoid, and not a refusal that
//! would block an import over an ordinate most consumers will never use.

use energyml_eml_2_3 as eml;

use crate::operation::{Error, Identity, NewObject};

/// What the coordinates will be written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The degrees as they stand, under a `Geographic2dCrs`.
    ///
    /// Nothing moves, so nothing can be lost — and the horizontal unit is a
    /// degree while the vertical is a metre, which no reader can reconcile.
    Geographic(i64),
    /// Metres, under a `ProjectedCrs`, reprojected from WGS 84.
    ///
    /// A transformation, so it is where coordinates can quietly move — which is
    /// why the EPSG code is the caller's and never guessed, and why
    /// `energyml-proj` (agreeing with PROJ 9.5.1 to the millimetre) does the
    /// arithmetic rather than four lines written here.
    Projected(i64),
}

impl Target {
    /// The EPSG code, whichever kind it is.
    pub fn epsg(&self) -> i64 {
        match self {
            Target::Geographic(code) | Target::Projected(code) => *code,
        }
    }
}

/// Which vertical system the elevations are in, when anybody knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vertical {
    /// A named EPSG vertical system — `5720` for NGF-IGN69, `5714` for mean sea
    /// level.
    Epsg(i64),
    /// Nobody said. Written as a `VerticalUnknownCrs` carrying the reason, so
    /// the package states its own ignorance instead of implying a datum.
    Unknown(String),
}

impl Default for Vertical {
    fn default() -> Self {
        Vertical::Unknown(
            "the GeoJSON does not state a vertical datum; RFC 7946 implies the WGS 84 ellipsoid \
             and producers commonly write a national levelling system instead"
                .to_string(),
        )
    }
}

impl Vertical {
    /// What names it in a derived uuid and in an activity: its code, or
    /// `unknown`. One spelling, where three callers each had their own `match`.
    pub fn key(&self) -> String {
        match self {
            Vertical::Epsg(code) => code.to_string(),
            Vertical::Unknown(_) => "unknown".to_string(),
        }
    }
}

/// Which way the local vertical axis counts, and in what unit.
///
/// The compound's `VerticalAxis` governs the local z — its sense, its unit and
/// whether it is a time — while the `VerticalCrs` gives the datum (the reading
/// rule the readers follow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Axis {
    /// The `VerticalCrs`'s own sense — **up**, since [`build`] writes every
    /// `VerticalCrs` as an elevation — in metres. The default: the z an
    /// operation writes keeps the meaning it had under the `ProjectedCrs`.
    Elevation,
    /// Down, in metres: a depth below the local origin.
    Depth,
    /// Down, in a time unit (`ms`, `s`): a travel time, `IsTime` true. The
    /// `VerticalCrs` stays a length — a top-level one cannot be anything else,
    /// its `uom` is a `LengthUomExt` (`CRS.xsd` l. 677) — and says the datum
    /// the times are counted from.
    Time(String),
}

impl Axis {
    /// The word the `crs` operation takes and a derived uuid carries.
    pub fn word(&self) -> &'static str {
        match self {
            Axis::Elevation => "elevation",
            Axis::Depth => "depth",
            Axis::Time(_) => "time",
        }
    }

    /// The unit the axis counts in.
    pub fn uom(&self) -> &str {
        match self {
            Axis::Elevation | Axis::Depth => "m",
            Axis::Time(unit) => unit,
        }
    }

    /// Whether the local z is a travel time.
    pub fn is_time(&self) -> bool {
        matches!(self, Axis::Time(_))
    }
}

/// The local engineering frame set on the projected system — what the
/// `LocalEngineering2dCrs` and the `LocalEngineeringCompoundCrs` say.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    /// Where the local origin sits in the projected system — easting, then
    /// northing, in its metres (`OriginProjectedCoordinate1` and `…2`).
    pub origin: [f64; 2],
    /// The rotation of the local Y axis, **in degrees, clockwise from grid
    /// north** — `Azimuth`, written with `AzimuthReference` = `grid north`.
    ///
    /// The eml 2.3 schema documents neither element. The sense is the one
    /// RESQML 2.0.1 documents for the `ArealRotation` the azimuth replaced —
    /// *"The rotation of the local Y axis relative to the projected Y axis. A
    /// positive value indicates a clockwise rotation from the projected Y
    /// axis"* (RESQML 2.0.1 `Common.xsd`, `AbstractLocal3dCrs`, l. 136–145) —
    /// and the projected Y axis of a projected system *is* grid north, the
    /// reading a version translation already writes
    /// and the one `energyml_domain::crs::read_with` inverts.
    pub azimuth: f64,
    /// Which way the local z counts, and in what.
    pub axis: Axis,
    /// The vertical coordinate of the local origin **in the `VerticalCrs`** —
    /// its sense and its unit, not the local axis's: *"Vertical coordinate of
    /// the origin of the local engineering CRS in the base vertical CRS
    /// (consequently in the uom of the base vertical CRS)"* (`CRS.xsd`
    /// l. 517–520).
    pub origin_vertical: f64,
}

impl Local {
    /// The identity: what every created frame is unless somebody asks the
    /// `crs` operation for more.
    pub fn identity() -> Local {
        Local {
            origin: [0.0, 0.0],
            azimuth: 0.0,
            axis: Axis::Elevation,
            origin_vertical: 0.0,
        }
    }

    /// Whether this frame moves nothing.
    pub fn is_identity(&self) -> bool {
        *self == Local::identity()
    }
}

impl Default for Local {
    fn default() -> Self {
        Local::identity()
    }
}

/// The horizontal half, whichever kind the target asked for.
#[derive(Debug, Clone)]
pub enum Horizontal {
    /// Degrees.
    Geographic(eml::Geographic2dCrs),
    /// Metres.
    Projected(eml::ProjectedCrs),
}

impl Horizontal {
    /// Its uuid.
    pub fn uuid(&self) -> &str {
        match self {
            Horizontal::Geographic(crs) => &crs.uuid,
            Horizontal::Projected(crs) => &crs.uuid,
        }
    }

    /// Its title.
    pub fn title(&self) -> &str {
        match self {
            Horizontal::Geographic(crs) => &crs.citation.title,
            Horizontal::Projected(crs) => &crs.citation.title,
        }
    }

    /// The qualified type a reference to it must carry.
    pub fn qualified_type(&self) -> &'static str {
        match self {
            Horizontal::Geographic(_) => "eml23.Geographic2dCrs",
            Horizontal::Projected(_) => "eml23.ProjectedCrs",
        }
    }

    fn json(&self) -> serde_json::Value {
        match self {
            Horizontal::Geographic(crs) => energyml_core::json::write_value(crs),
            Horizontal::Projected(crs) => energyml_core::json::write_value(crs),
        }
    }

    fn boxed(self) -> Box<dyn energyml_core::registry::DynObject> {
        match self {
            Horizontal::Geographic(crs) => Box::new(crs),
            Horizontal::Projected(crs) => Box::new(crs),
        }
    }
}

/// The local frame over metres: the plane and the compound that joins it to
/// the vertical.
#[derive(Debug, Clone)]
pub struct LocalFrame {
    /// Origin, azimuth, horizontal axes — and the projected system **embedded**
    /// as its `OriginProjectedCrs` (`CRS.xsd` l. 504 types it `ProjectedCrs`,
    /// not a reference): the same object as the horizontal half, uuid
    /// included, so the package says one system and not two.
    pub plane: eml::LocalEngineering2dCrs,
    /// The frame a geometry names.
    pub compound: eml::LocalEngineeringCompoundCrs,
}

/// The CRS objects a set of coordinates needs, and the one to point at.
#[derive(Debug, Clone)]
pub struct Built {
    /// The horizontal half, carrying the EPSG code.
    pub horizontal: Horizontal,
    /// The vertical half — referenced by the compound over metres, and by
    /// nothing in degrees (the named exception, see the module doc).
    pub vertical: eml::VerticalCrs,
    /// The local 2D and the compound — `None` in degrees, where the schema
    /// offers none.
    pub local: Option<LocalFrame>,
}

/// The role, in a plan, of the object a geometry names — the compound, or the
/// `Geographic2dCrs` in degrees. **Always this one**, whatever the target, so
/// that a caller looking for *the frame* of a run finds it under one name.
pub const FRAME_ROLE: &str = "crs";

impl Built {
    /// Every object, as objects to put in a store — horizontal, vertical, then
    /// the plane and the compound when there are.
    pub fn objects(self) -> Vec<Box<dyn energyml_core::registry::DynObject>> {
        let mut out = vec![
            self.horizontal.boxed(),
            Box::new(self.vertical) as Box<dyn energyml_core::registry::DynObject>,
        ];
        if let Some(local) = self.local {
            out.push(Box::new(local.plane));
            out.push(Box::new(local.compound));
        }
        out
    }

    /// The reference a geometry carries: the compound, or the
    /// `Geographic2dCrs` in degrees.
    pub fn reference(&self) -> eml::DataObjectReference {
        match &self.local {
            Some(local) => reference_to(
                &local.compound.uuid,
                "eml23.LocalEngineeringCompoundCrs",
                &local.compound.citation.title,
            ),
            None => reference_to(
                self.horizontal.uuid(),
                self.horizontal.qualified_type(),
                self.horizontal.title(),
            ),
        }
    }

    /// A reference to the vertical half — what a `ReferencePointInACrs` names.
    pub fn vertical_reference(&self) -> eml::DataObjectReference {
        reference_to(
            &self.vertical.uuid,
            "eml23.VerticalCrs",
            &self.vertical.citation.title,
        )
    }

    /// Give the frame a geometry names its title — the compound, or the
    /// horizontal half in degrees. The other objects keep the titles that say
    /// what they are (`EPSG:2154`, `EPSG:5720`).
    pub fn retitle(&mut self, title: &str) {
        match (&mut self.local, &mut self.horizontal) {
            (Some(local), _) => local.compound.citation.title = title.to_string(),
            (None, Horizontal::Geographic(crs)) => crs.citation.title = title.to_string(),
            (None, Horizontal::Projected(crs)) => crs.citation.title = title.to_string(),
        }
    }

    /// The objects as a plan writes them, **in dependency order** — what is
    /// referenced before what references it — each with its role:
    ///
    /// | object | role |
    /// |---|---|
    /// | `ProjectedCrs` | `projected-crs` |
    /// | `Geographic2dCrs` (degrees) | `crs` |
    /// | `VerticalCrs` | `vertical-crs` |
    /// | `LocalEngineering2dCrs` | `local-2d-crs` |
    /// | `LocalEngineeringCompoundCrs` | `crs` |
    ///
    /// [`FRAME_ROLE`] is always the object a geometry names.
    pub fn new_objects(&self) -> Vec<NewObject> {
        let horizontal_role = if self.local.is_some() {
            "projected-crs"
        } else {
            FRAME_ROLE
        };
        let mut out = vec![
            NewObject {
                uuid: self.horizontal.uuid().to_string(),
                qualified_type: self.horizontal.qualified_type().to_string(),
                role: horizontal_role.to_string(),
                title: self.horizontal.title().to_string(),
                json: self.horizontal.json(),
            },
            NewObject {
                uuid: self.vertical.uuid.clone(),
                qualified_type: "eml23.VerticalCrs".to_string(),
                role: "vertical-crs".to_string(),
                title: self.vertical.citation.title.clone(),
                json: energyml_core::json::write_value(&self.vertical),
            },
        ];
        if let Some(local) = &self.local {
            out.push(NewObject {
                uuid: local.plane.uuid.clone(),
                qualified_type: "eml23.LocalEngineering2dCrs".to_string(),
                role: "local-2d-crs".to_string(),
                title: local.plane.citation.title.clone(),
                json: energyml_core::json::write_value(&local.plane),
            });
            out.push(NewObject {
                uuid: local.compound.uuid.clone(),
                qualified_type: "eml23.LocalEngineeringCompoundCrs".to_string(),
                role: FRAME_ROLE.to_string(),
                title: local.compound.citation.title.clone(),
                json: energyml_core::json::write_value(&local.compound),
            });
        }
        out
    }
}

/// The uuids of the objects [`build`] makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    /// The `ProjectedCrs` or `Geographic2dCrs`.
    pub horizontal: String,
    /// The `VerticalCrs`.
    pub vertical: String,
    /// The `LocalEngineering2dCrs` and the `LocalEngineeringCompoundCrs` —
    /// `None` in degrees, where there is none to name.
    pub local: Option<(String, String)>,
}

/// The uuids of a frame, **in the plan's order**, and `rank` moved past them.
///
/// Derived, the horizontal and the vertical keep the names they always had —
/// [`crate::naming::crs`], from their codes alone — so a package written before
/// the compound existed names the same two systems. The plane and the compound
/// are derived from **everything that tells them apart**: the plane from the
/// projected code, its origin and its azimuth; the compound from the plane, the
/// vertical system, the axis — its sense, unit and time flag — and the vertical
/// origin. Two runs of `crs` on one EPSG with two axes therefore share the
/// projected, the vertical and the plane, and differ by the compound only.
///
/// Given, they are taken in the order a plan lists them: two in degrees, four
/// over metres.
pub fn names(
    identity: &Identity,
    target: &Target,
    vertical: &Vertical,
    local: &Local,
    rank: &mut usize,
) -> Result<Names, Error> {
    let projected = matches!(target, Target::Projected(_));
    let names = match identity {
        Identity::Derived { .. } => {
            let (horizontal, vertical_uuid) = crate::naming::crs(target.epsg(), &vertical.key());
            let local = projected.then(|| {
                let plane = plane_seed(target.epsg(), local);
                let compound = compound_seed(&plane, vertical, local);
                (derive(&plane), derive(&compound))
            });
            Names {
                horizontal,
                vertical: vertical_uuid,
                local,
            }
        }
        Identity::Given(_) => {
            let horizontal = identity.uuid("", *rank)?;
            let vertical_uuid = identity.uuid("", *rank + 1)?;
            let local = if projected {
                Some((identity.uuid("", *rank + 2)?, identity.uuid("", *rank + 3)?))
            } else {
                None
            };
            Names {
                horizontal,
                vertical: vertical_uuid,
                local,
            }
        }
    };
    *rank += if projected { 4 } else { 2 };
    Ok(names)
}

/// A uuid in the crate's namespace — the one [`crate::naming`] hangs every
/// derived name from, so the two new names live beside the two old ones.
fn derive(name: &str) -> String {
    uuid::Uuid::new_v5(&crate::naming::NAMESPACE, name.as_bytes()).to_string()
}

/// A number as a seed spells it: the shortest text that reads back to it, and
/// `-0` folded into `0` — the two place a frame identically.
fn seed_number(value: f64) -> String {
    format!("{}", value + 0.0)
}

fn plane_seed(epsg: i64, local: &Local) -> String {
    format!(
        "crs:local2d:{epsg}:{}:{}:{}",
        seed_number(local.origin[0]),
        seed_number(local.origin[1]),
        seed_number(local.azimuth)
    )
}

fn compound_seed(plane: &str, vertical: &Vertical, local: &Local) -> String {
    format!(
        "crs:compound:{plane}:{}:{}:{}:{}",
        vertical.key(),
        local.axis.word(),
        local.axis.uom(),
        seed_number(local.origin_vertical)
    )
}

/// Build the objects for `target` horizontally, `vertical` elevations, and —
/// over metres — the `local` frame joining them.
///
/// `names` are supplied rather than drawn: the whole import is reproducible, so
/// running it twice on one file must name the same objects. See [`names`] and
/// [`crate::naming`].
///
/// `citation` is given each object's own title — `EPSG:2154`, `EPSG:5720`, the
/// plane and the compound spelled from what they are — and [`Built::retitle`]
/// renames the frame a geometry names when a caller has a name for it.
///
/// # What is refused, and why here
///
/// - a local frame that is not the identity **in degrees**: its origin would be
///   an easting in a system that has none (`CRS.xsd` l. 504);
/// - a time axis whose unit is not a time, or not one the schema spells —
///   `IsTime` over metres would be a travel time measured with a ruler;
/// - a number that is not one.
///
/// Refused here rather than by each caller, because this is where the objects
/// are made: a second place deciding it would be a second answer.
pub fn build(
    target: &Target,
    vertical: &Vertical,
    local: &Local,
    names: &Names,
    citation: impl Fn(&str) -> eml::Citation,
) -> Result<Built, String> {
    let epsg = target.epsg();
    for (what, value) in [
        ("originEasting", local.origin[0]),
        ("originNorthing", local.origin[1]),
        ("azimuth", local.azimuth),
        ("originVertical", local.origin_vertical),
    ] {
        if !value.is_finite() {
            return Err(format!("`{what}` is {value}, which places nothing"));
        }
    }
    if matches!(target, Target::Geographic(_)) && !local.is_identity() {
        return Err(format!(
            "EPSG:{epsg} measures degrees, and a local frame — an origin, an azimuth, a depth or time axis, a vertical origin — sits on a projected system: `LocalEngineering2dCrs.OriginProjectedCrs` is a `ProjectedCrs` (CRS.xsd l. 504), so there is no compound over degrees to carry it"
        ));
    }
    let axis_uom = local_axis_uom(&local.axis)?;

    let horizontal = match target {
        Target::Geographic(_) => Horizontal::Geographic(eml::Geographic2dCrs {
            uuid: names.horizontal.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(&format!("EPSG:{epsg}")),
            abstract_geographic2d_crs: Some(eml::AbstractGeographic2dCrs::GeodeticEpsgCrs(
                eml::GeodeticEpsgCrs { epsg_code: epsg },
            )),
            ..Default::default()
        }),
        Target::Projected(_) => Horizontal::Projected(eml::ProjectedCrs {
            uuid: names.horizontal.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(&format!("EPSG:{epsg}")),
            // **Easting then northing**, which is the order the reprojector
            // returns and therefore the order the array holds. Writing the
            // authority's order here while the numbers are in the other one is
            // the failure that puts a model in the sea with every coordinate
            // still looking plausible.
            axis_order: eml::AxisOrder2d::EastingNorthing,
            // Stated, unlike the geographic case where the schema has nowhere to
            // put it. This is the whole gain: a reader now knows the horizontal
            // and the vertical are the same unit.
            uom: Some(eml::LengthUomExt::M),
            abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(
                eml::ProjectedEpsgCrs { epsg_code: epsg },
            )),
            ..Default::default()
        }),
    };

    let vertical_crs = eml::VerticalCrs {
        uuid: names.vertical.clone(),
        schema_version: "2.3".to_string(),
        citation: citation(&match vertical {
            Vertical::Epsg(code) => format!("EPSG:{code}"),
            Vertical::Unknown(_) => "vertical, datum not stated".to_string(),
        }),
        // Up, because a GeoJSON's third ordinate is an *elevation* — RFC 7946
        // §3.1.1 says height, and the BRGM exports carry ground altitude. A
        // depth-positive axis here would put every point below the datum.
        direction: eml::VerticalDirection::Up,
        // Metres: the format states no unit and the one it implies is the
        // metre. A file in feet would be a file that broke RFC 7946, and it
        // would break it silently — but nothing here can detect that, so the
        // assumption is written down rather than hidden.
        uom: eml::LengthUomExt::M,
        abstract_vertical_crs: Some(match vertical {
            Vertical::Epsg(code) => {
                eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs { epsg_code: *code })
            }
            Vertical::Unknown(why) => {
                eml::AbstractVerticalCrs::VerticalUnknownCrs(eml::VerticalUnknownCrs { unknown: why.clone() })
            }
        }),
        ..Default::default()
    };

    let local_frame = match (&horizontal, &names.local) {
        (Horizontal::Geographic(_), _) => None,
        (Horizontal::Projected(_), None) => {
            return Err(format!(
                "EPSG:{epsg} is written in metres, so its frame is a compound, and no uuid was given for its local 2D and its compound"
            ));
        }
        (Horizontal::Projected(projected), Some((plane_uuid, compound_uuid))) => {
            let plane_title = if local.origin == [0.0, 0.0] && local.azimuth == 0.0 {
                format!("EPSG:{epsg} local")
            } else {
                format!(
                    "EPSG:{epsg} local, origin {} {}, azimuth {}°",
                    seed_number(local.origin[0]),
                    seed_number(local.origin[1]),
                    seed_number(local.azimuth)
                )
            };
            let plane = eml::LocalEngineering2dCrs {
                uuid: plane_uuid.clone(),
                schema_version: "2.3".to_string(),
                citation: citation(&plane_title),
                azimuth: eml::PlaneAngleMeasureExt {
                    value: local.azimuth,
                    uom: eml::PlaneAngleUomExt::Dega,
                },
                // Grid north: the only reference an azimuth can be written in
                // without a convergence angle this crate does not have — and
                // the only one `energyml_domain::crs` places a non-zero azimuth
                // in. The NGS definition, in `CommonEnumerations.xsd` l. 974:
                // *"grid north - The direction of the north lines on a map
                // projection"*.
                azimuth_reference: eml::NorthReferenceKind::GridNorth,
                origin_projected_coordinate1: local.origin[0],
                origin_projected_coordinate2: local.origin[1],
                // East then north, in metres — the projected system's own axes
                // (`AxisOrder` easting northing above), which the local ones
                // follow before the azimuth turns them.
                horizontal_axes: eml::HorizontalAxes {
                    direction1: eml::AxisDirectionKind::East,
                    direction2: eml::AxisDirectionKind::North,
                    uom: eml::LengthAndTimeUomExt::M,
                    is_time: false,
                },
                origin_projected_crs: projected.clone(),
                ..Default::default()
            };
            let compound_title = match (vertical, &local.axis) {
                (Vertical::Epsg(code), Axis::Elevation) => format!("EPSG:{epsg} + EPSG:{code}"),
                (Vertical::Epsg(code), axis) => {
                    format!("EPSG:{epsg} + EPSG:{code}, {} in {}", axis.word(), axis.uom())
                }
                (Vertical::Unknown(_), Axis::Elevation) => format!("EPSG:{epsg} + unknown vertical"),
                (Vertical::Unknown(_), axis) => format!(
                    "EPSG:{epsg} + unknown vertical, {} in {}",
                    axis.word(),
                    axis.uom()
                ),
            };
            let compound = eml::LocalEngineeringCompoundCrs {
                uuid: compound_uuid.clone(),
                schema_version: "2.3".to_string(),
                citation: citation(&compound_title),
                vertical_crs: reference_to(
                    &vertical_crs.uuid,
                    "eml23.VerticalCrs",
                    &vertical_crs.citation.title,
                ),
                origin_vertical_coordinate: local.origin_vertical,
                vertical_axis: eml::VerticalAxis {
                    direction: match local.axis {
                        // **The `VerticalCrs`'s own sense**, read off the object
                        // just made rather than restated: an elevation axis is
                        // one that agrees with its datum.
                        Axis::Elevation => vertical_crs.direction.clone(),
                        Axis::Depth | Axis::Time(_) => eml::VerticalDirection::Down,
                    },
                    uom: axis_uom,
                    is_time: local.axis.is_time(),
                },
                local_engineering2d_crs: reference_to(
                    &plane.uuid,
                    "eml23.LocalEngineering2dCrs",
                    &plane.citation.title,
                ),
                ..Default::default()
            };
            Some(LocalFrame { plane, compound })
        }
    };

    Ok(Built {
        horizontal,
        vertical: vertical_crs,
        local: local_frame,
    })
}

/// The unit of the local vertical axis, as the schema spells it — or why not.
fn local_axis_uom(axis: &Axis) -> Result<eml::LengthAndTimeUomExt, String> {
    use energyml_core::xml::XmlText;
    match axis {
        Axis::Elevation | Axis::Depth => Ok(eml::LengthAndTimeUomExt::M),
        Axis::Time(unit) => {
            if energyml_domain::units::Dimension::of(unit) != Some(energyml_domain::units::Dimension::Time) {
                return Err(format!(
                    "`timeUom` is `{unit}`, which is no time unit this build knows — a time axis counts in `ms` or `s`"
                ));
            }
            eml::LengthAndTimeUomExt::from_xml_text(unit).map_err(|why| {
                format!("`timeUom` is `{unit}`, which `LengthAndTimeUomExt` does not spell: {why}")
            })
        }
    }
}

/// A `DataObjectReference` to something this module just built.
///
/// Local to this module on purpose: a reference to an object of *the package*
/// has to be resolved against the package, and that is the caller's job. These
/// objects point at each other and at nothing else, so their content types are
/// known without asking anybody.
pub fn reference_to(uuid: &str, qualified_type: &str, title: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

/// A `DataObjectReference` to an object **named by a `Ref`** — the version it
/// declares included, so that a package holding the object in two versions is
/// pointed at the one meant. The sibling of [`reference_to`] for what comes from
/// the package; `reference_to` stays for what this crate has just built, which
/// declares none.
pub fn reference_of(one: &crate::pieces::Ref) -> eml::DataObjectReference {
    eml::DataObjectReference {
        object_version: one.object_version.clone(),
        ..reference_to(&one.uuid, &one.qualified_type, &one.title)
    }
}

// ---------------------------------------------------------------------------
// What a frame named in the package is
// ---------------------------------------------------------------------------

/// The systems a geometry may name as its `LocalCrs`, **whole** — what
/// `Need::Crs` offers and what `support::frame_in` accepts without a refusal.
///
/// **One list**, read by the operations, a front end's CRS field and its
/// operation catalogue, so that what a form offers is what an
/// operation takes. Each is classified by [`frame_kind`]:
///
/// - the compound, what RESQML 2.2 associates (TRG §3.1, p. 22);
/// - RESQML 2.0.1's two local 3D systems — whole in their release, and
///   blocked for writing by the operations, which write 2.2;
/// - a `ProjectedCrs` named directly — the BRGM's surfaces, our packages
///   written by earlier versions — **accepted with a note**;
/// - a `Geographic2dCrs` — degrees, the named exception, with its note.
pub const FRAMES: &[&str] = &[
    "eml23.LocalEngineeringCompoundCrs",
    "eml23.ProjectedCrs",
    "eml23.Geographic2dCrs",
    "resqml20.obj_LocalDepth3dCrs",
    "resqml20.obj_LocalTime3dCrs",
];

/// What kind of frame a named system is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// An eml 2.3 `LocalEngineeringCompoundCrs` — what a geometry names.
    Compound,
    /// A RESQML 2.0.1 `LocalDepth3dCrs` or `LocalTime3dCrs`: whole in its
    /// release, and of another release than the objects written here.
    Legacy,
    /// A `ProjectedCrs` named directly — accepted with a note (decision c).
    Projected,
    /// A `Geographic2dCrs` — degrees, the named exception (decision a).
    Geographic,
}

/// The frames of a package a geometry in metres of `projected` can be written
/// in **instead of creating one**: checking, before saving, whether a
/// compatible projected / vertical / compound CRS already exists in the target
/// package avoids recreating objects that already carry the same information.
///
/// Compatible is what [`build`] would write for these codes, the sense of the
/// vertical axis aside: a `LocalEngineeringCompoundCrs` naming that projected
/// EPSG code, with no offset and no rotation (its numbers are the projected
/// ones), a **length** axis (a time axis would read a height as a travel time)
/// and metres on both axes. The vertical system is the caller's to match —
/// `vertical_epsg`, `None` when the file names none. A frame counting **down**
/// is compatible — a height is written in it as its depth, by
/// `energyml_domain::crs::Crs::from_projected` — and comes after one counting
/// up, which is what would have been created. Then by title, for a stable
/// order.
pub fn compatible_frames(candidates: &[crate::pieces::Known], projected: i64) -> Vec<&crate::pieces::Known> {
    use energyml_domain::crs::{VerticalDirection, VerticalQuantity};
    let metres = |uom: &Option<String>| uom.as_deref().is_some_and(|u| u.eq_ignore_ascii_case("m"));
    let mut out: Vec<&crate::pieces::Known> = candidates
        .iter()
        .filter(|known| {
            let Some(reference) = &known.reference else {
                return false;
            };
            let Some(crs) = &known.placement else { return false };
            matches!(frame_kind(&reference.qualified_type), Ok(FrameKind::Compound))
                && known.crs_epsg == Some(projected)
                && crs.x_offset.abs() < 1e-9
                && crs.y_offset.abs() < 1e-9
                && crs.z_offset.abs() < 1e-9
                && crs.areal_rotation.abs() < 1e-12
                && crs.vertical_quantity == VerticalQuantity::Length
                && metres(&crs.horizontal_uom)
                && metres(&crs.vertical_uom)
        })
        .collect();
    let down = |known: &crate::pieces::Known| {
        known
            .placement
            .as_ref()
            .is_some_and(|crs| crs.vertical_direction == VerticalDirection::Down)
    };
    let title = |known: &crate::pieces::Known| {
        known
            .reference
            .as_ref()
            .map(|r| r.title.clone())
            .unwrap_or_default()
    };
    out.sort_by(|a, b| down(a).cmp(&down(b)).then_with(|| title(a).cmp(&title(b))));
    out
}

/// What a named system is, or **why it is no frame** — by its class name,
/// ignoring the package prefix, the case and the `obj_` RESQML 2.0.1 spells.
///
/// The refusal names the half it is and what to name instead: a `VerticalCrs`
/// read on its own gives a frame with no horizontal, a `LocalEngineering2dCrs`
/// one with no vertical — each a perfectly readable half, which is exactly the
/// sort of thing that looks like a complete answer.
pub fn frame_kind(qualified_type: &str) -> Result<FrameKind, String> {
    let class = qualified_type
        .rsplit('.')
        .next()
        .unwrap_or(qualified_type)
        .to_ascii_lowercase();
    let class = class.strip_prefix("obj_").unwrap_or(&class);
    match class {
        "localengineeringcompoundcrs" => Ok(FrameKind::Compound),
        "localdepth3dcrs" | "localtime3dcrs" => Ok(FrameKind::Legacy),
        "projectedcrs" => Ok(FrameKind::Projected),
        "geographic2dcrs" => Ok(FrameKind::Geographic),
        "verticalcrs" => Err(format!(
            "`{qualified_type}` is the vertical half of a system, with no horizontal: name the `LocalEngineeringCompoundCrs` that joins it to a plane (RESQML TRG 2.2 §3.1, p. 22)"
        )),
        "localengineering2dcrs" => Err(format!(
            "`{qualified_type}` is the horizontal half of a local system, with no vertical: name the `LocalEngineeringCompoundCrs` that joins it to a `VerticalCrs` (RESQML TRG 2.2 §3.1, p. 22)"
        )),
        _ if class.starts_with("referencepoint") => Err(format!(
            "`{qualified_type}` is a reference point — where measured depths start — not a coordinate system: name the `LocalEngineeringCompoundCrs` it sits in"
        )),
        _ if qualified_type.is_empty() => {
            Err("the reference does not say what it names (no `QualifiedType`), so whether it is a whole system cannot be told".to_string())
        }
        _ => Err(format!(
            "`{qualified_type}` is not a coordinate system a geometry can name: one of {}",
            FRAMES.join(", ")
        )),
    }
}

/// A metric system the data itself determines: its WGS 84 UTM zone.
///
/// # Why this is a derivation and not a guess
///
/// The repository's rule is that a projection is never chosen for somebody,
/// because a transformation moves every coordinate and the choice of datum is
/// *data*, not arithmetic. This does not break that rule, and the difference is
/// worth stating: a UTM zone is **a function of the longitude**, the datum is
/// the one RFC 7946 already fixes for the file (WGS 84), and what comes out is
/// written into the package as its own EPSG code. Nothing is implied and
/// nothing is hidden.
///
/// So it is offered as a **suggestion** — a form pre-fills it, a person sees it
/// and can replace it with their national system — never as a default that
/// applies itself. The source system of a GeoJSON is known, by the format; the
/// target is a
/// decision, and this is the one proposal that can be made from the numbers
/// alone.
///
/// # The exceptions are the published ones
///
/// South-west Norway widens zone 32, and Svalbard uses four zones instead of
/// seven. They are in the standard, they are two `if`s, and leaving them out
/// would put a Norwegian well one zone off — still metric, still drawable, and
/// not what any neighbouring dataset uses.
pub fn utm_epsg_of(lon: f64, lat: f64) -> Option<i64> {
    if !lon.is_finite() || !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return None;
    }
    let lon = ((lon + 180.0).rem_euclid(360.0)) - 180.0;
    let mut zone = ((lon + 180.0) / 6.0).floor() as i64 + 1;
    if (56.0..64.0).contains(&lat) && (3.0..12.0).contains(&lon) {
        zone = 32;
    }
    if (72.0..84.0).contains(&lat) && (0.0..42.0).contains(&lon) {
        zone = match lon {
            l if l < 9.0 => 31,
            l if l < 21.0 => 33,
            l if l < 33.0 => 35,
            _ => 37,
        };
    }
    let zone = zone.clamp(1, 60);
    Some(if lat < 0.0 { 32_700 + zone } else { 32_600 + zone })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn citation(title: &str) -> eml::Citation {
        eml::Citation {
            title: title.to_string(),
            originator: "test".to_string(),
            creation: "2026-09-07T00:00:00Z".to_string(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        }
    }

    fn named(local: bool) -> Names {
        Names {
            horizontal: "00000000-0000-4000-8000-00000000000a".to_string(),
            vertical: "00000000-0000-4000-8000-00000000000b".to_string(),
            local: local.then(|| {
                (
                    "00000000-0000-4000-8000-00000000000c".to_string(),
                    "00000000-0000-4000-8000-00000000000d".to_string(),
                )
            }),
        }
    }

    fn built(vertical: &Vertical) -> Built {
        build(
            &Target::Geographic(4326),
            vertical,
            &Local::identity(),
            &named(false),
            citation,
        )
        .expect("degrees")
    }

    fn metres(local: &Local) -> Built {
        build(
            &Target::Projected(2154),
            &Vertical::Epsg(5720),
            local,
            &named(true),
            citation,
        )
        .expect("metres")
    }

    #[test]
    fn the_epsg_code_lands_where_the_schema_can_hold_it() {
        // `Geographic3dCrs` cannot: it is empty in the standard. The horizontal
        // half is where 4326 goes, and this test fails if anybody "simplifies"
        // the objects back into one.
        let made = built(&Vertical::Epsg(5720));
        let Horizontal::Geographic(crs) = &made.horizontal else {
            panic!("asked for degrees, got metres")
        };
        let eml::AbstractGeographic2dCrs::GeodeticEpsgCrs(epsg) = crs
            .abstract_geographic2d_crs
            .as_ref()
            .expect("the horizontal half carries a definition")
        else {
            panic!("expected an EPSG geodetic CRS")
        };
        assert_eq!(epsg.epsg_code, 4326);
    }

    #[test]
    fn degrees_are_the_named_exception_and_name_their_geographic_half() {
        // **This test used to say the opposite** — *a geometry names the
        // horizontal half because there is no compound to name* — and it was
        // right only in degrees. Over metres the compound exists and is what
        // RESQML associates (TRG 2.2 §3.1, p. 22), which the next test holds.
        //
        // In degrees nothing changed, and that is the named exception: EML 2.3 has no
        // compound over a `Geographic2dCrs` (`OriginProjectedCrs` is a
        // `ProjectedCrs`, CRS.xsd l. 504), so the geometry still names the
        // geographic half, and the vertical is still referenced by nothing.
        let made = built(&Vertical::Epsg(5720));
        let reference = made.reference();
        assert_eq!(reference.uuid, made.horizontal.uuid());
        assert_eq!(reference.qualified_type, "eml23.Geographic2dCrs");
        assert!(made.local.is_none(), "no compound over degrees");
        assert_eq!(made.clone().objects().len(), 2);
        // And the frame's role is the one name a caller looks for.
        let objects = made.new_objects();
        assert_eq!(
            objects.iter().map(|o| o.role.as_str()).collect::<Vec<_>>(),
            [FRAME_ROLE, "vertical-crs"]
        );
    }

    #[test]
    fn over_metres_a_geometry_names_the_compound_that_joins_the_two_halves() {
        // RESQML TRG v2.2, §3.1, p. 22: *« From: AbstractGeometry.LocalCrs — To:
        // LocalEngineeringCompoundCrs »*. To break: `reference()` pointing at the
        // `ProjectedCrs` again, which leaves the `VerticalCrs` referenced by
        // nothing.
        let made = metres(&Local::identity());
        let local = made.local.as_ref().expect("a compound over metres");
        let reference = made.reference();
        assert_eq!(reference.qualified_type, "eml23.LocalEngineeringCompoundCrs");
        assert_eq!(reference.uuid, local.compound.uuid);
        // The compound names both halves — the vertical directly, the
        // horizontal through the plane — which is the whole point of it.
        assert_eq!(local.compound.vertical_crs.uuid, made.vertical.uuid);
        assert_eq!(local.compound.vertical_crs.qualified_type, "eml23.VerticalCrs");
        assert_eq!(local.compound.local_engineering2d_crs.uuid, local.plane.uuid);
        assert_eq!(
            local.compound.local_engineering2d_crs.qualified_type,
            "eml23.LocalEngineering2dCrs"
        );
        // The plane **embeds** the projected system — CRS.xsd l. 504 types
        // `OriginProjectedCrs` as a `ProjectedCrs`, not a reference — and it
        // is the same object as the top-level one, uuid and code included.
        let Horizontal::Projected(projected) = &made.horizontal else {
            panic!("metres")
        };
        assert_eq!(local.plane.origin_projected_crs.uuid, projected.uuid);
        assert_eq!(
            local.plane.origin_projected_crs.abstract_projected_crs,
            projected.abstract_projected_crs
        );
        // Four parts, in the order a plan writes them, `crs` the compound.
        let objects = made.new_objects();
        let roles: Vec<(&str, &str)> = objects
            .iter()
            .map(|o| (o.role.as_str(), o.qualified_type.as_str()))
            .collect();
        assert_eq!(
            roles,
            [
                ("projected-crs", "eml23.ProjectedCrs"),
                ("vertical-crs", "eml23.VerticalCrs"),
                ("local-2d-crs", "eml23.LocalEngineering2dCrs"),
                (FRAME_ROLE, "eml23.LocalEngineeringCompoundCrs"),
            ]
        );
    }

    #[test]
    fn the_default_compound_is_the_identity_so_nothing_written_moves() {
        // Origin 0/0, azimuth 0 from grid north, the axis in the vertical CRS's
        // own sense, metres, not a time, origin 0 in the vertical system. Under
        // it an easting *is* an easting — the z an operation writes keeps the
        // meaning it had under the `ProjectedCrs`.
        // To break: a default axis `down`, which would turn every elevation an
        // operation writes into a depth.
        let made = metres(&Local::identity());
        let local = made.local.as_ref().unwrap();
        assert_eq!(local.plane.origin_projected_coordinate1, 0.0);
        assert_eq!(local.plane.origin_projected_coordinate2, 0.0);
        assert_eq!(local.plane.azimuth.value, 0.0);
        assert_eq!(local.plane.azimuth.uom, eml::PlaneAngleUomExt::Dega);
        assert_eq!(local.plane.azimuth_reference, eml::NorthReferenceKind::GridNorth);
        assert_eq!(
            local.plane.horizontal_axes.direction1,
            eml::AxisDirectionKind::East
        );
        assert_eq!(
            local.plane.horizontal_axes.direction2,
            eml::AxisDirectionKind::North
        );
        assert!(!local.plane.horizontal_axes.is_time);
        assert_eq!(local.compound.vertical_axis.direction, made.vertical.direction);
        assert_eq!(local.compound.vertical_axis.uom, eml::LengthAndTimeUomExt::M);
        assert!(!local.compound.vertical_axis.is_time);
        assert_eq!(local.compound.origin_vertical_coordinate, 0.0);
    }

    #[test]
    fn a_depth_and_a_time_axis_count_down_and_the_time_one_says_so() {
        // The same vertical system, an
        // axis `down` in metres, and one `down` in milliseconds with `IsTime`.
        // The `VerticalCrs` stays up and a length — a top-level one cannot be a
        // time (CRS.xsd l. 677 types its uom `LengthUomExt`).
        let depth = metres(&Local {
            axis: Axis::Depth,
            ..Local::identity()
        });
        let axis = &depth.local.as_ref().unwrap().compound.vertical_axis;
        assert_eq!(
            (axis.direction.clone(), axis.uom.clone(), axis.is_time),
            (eml::VerticalDirection::Down, eml::LengthAndTimeUomExt::M, false)
        );
        assert_eq!(depth.vertical.direction, eml::VerticalDirection::Up);

        let time = metres(&Local {
            axis: Axis::Time("ms".into()),
            ..Local::identity()
        });
        let axis = &time.local.as_ref().unwrap().compound.vertical_axis;
        assert_eq!(
            (axis.direction.clone(), axis.uom.clone(), axis.is_time),
            (eml::VerticalDirection::Down, eml::LengthAndTimeUomExt::Ms, true)
        );
        assert_eq!(
            time.vertical.uom,
            eml::LengthUomExt::M,
            "the datum is still a length"
        );
    }

    #[test]
    fn an_origin_an_azimuth_and_a_vertical_origin_are_written_where_the_schema_puts_them() {
        let made = metres(&Local {
            origin: [650_000.0, 6_860_000.0],
            azimuth: 30.0,
            axis: Axis::Depth,
            origin_vertical: 35.0,
        });
        let local = made.local.as_ref().unwrap();
        assert_eq!(local.plane.origin_projected_coordinate1, 650_000.0);
        assert_eq!(local.plane.origin_projected_coordinate2, 6_860_000.0);
        assert_eq!(local.plane.azimuth.value, 30.0);
        // In the vertical CRS — its sense and unit (CRS.xsd l. 517–520) — so a
        // +35 is 35 m above the datum even though the local axis counts down.
        assert_eq!(local.compound.origin_vertical_coordinate, 35.0);
    }

    #[test]
    fn a_local_frame_over_degrees_and_a_time_counted_in_metres_are_refused() {
        // No compound over a `Geographic2dCrs` (CRS.xsd l. 504): an origin in
        // degrees would be an easting in a system that has none.
        let why = build(
            &Target::Geographic(4326),
            &Vertical::Epsg(5720),
            &Local {
                azimuth: 30.0,
                ..Local::identity()
            },
            &named(false),
            citation,
        )
        .expect_err("degrees have no local frame");
        assert!(why.contains("4326") && why.contains("504"), "{why}");
        // The identity over degrees is the named exception, not a refusal.
        assert!(
            build(
                &Target::Geographic(4326),
                &Vertical::Epsg(5720),
                &Local::identity(),
                &named(false),
                citation
            )
            .is_ok()
        );

        let why = build(
            &Target::Projected(2154),
            &Vertical::Epsg(5720),
            &Local {
                axis: Axis::Time("m".into()),
                ..Local::identity()
            },
            &named(true),
            citation,
        )
        .expect_err("a time axis in metres");
        assert!(why.contains("timeUom") && why.contains("`m`"), "{why}");
    }

    #[test]
    fn two_axes_on_one_code_share_three_systems_and_differ_by_the_compound() {
        // What a derived name is made of is what tells the object apart — and
        // nothing more: a depth frame and a time frame over Lambert-93 and
        // NGF-IGN69 are one projected, one vertical, one plane, two compounds.
        // To break: the axis left out of the compound's seed (the two collide),
        // or put in the plane's (two planes where there is one).
        let derived = Identity::derived();
        let name = |local: &Local| {
            let mut rank = 0;
            let out = names(
                &derived,
                &Target::Projected(2154),
                &Vertical::Epsg(5720),
                local,
                &mut rank,
            )
            .unwrap();
            assert_eq!(rank, 4, "four objects over metres");
            out
        };
        let depth = name(&Local {
            axis: Axis::Depth,
            ..Local::identity()
        });
        let time = name(&Local {
            axis: Axis::Time("ms".into()),
            ..Local::identity()
        });
        assert_eq!(depth.horizontal, time.horizontal);
        assert_eq!(depth.vertical, time.vertical);
        let (depth_plane, depth_compound) = depth.local.clone().unwrap();
        let (time_plane, time_compound) = time.local.clone().unwrap();
        assert_eq!(depth_plane, time_plane);
        assert_ne!(depth_compound, time_compound);
        // And the horizontal and vertical are the names they always had: a
        // package written before the compound names the same two systems.
        assert_eq!(
            (depth.horizontal.clone(), depth.vertical.clone()),
            crate::naming::crs(2154, "5720")
        );
        // An azimuth moves the plane, and the compound with it.
        let turned = name(&Local {
            azimuth: 30.0,
            axis: Axis::Depth,
            ..Local::identity()
        });
        assert_ne!(turned.local.as_ref().unwrap().0, depth_plane);
        assert_ne!(turned.local.as_ref().unwrap().1, depth_compound);
        // Degrees: two names, and the rank moves by two.
        let mut rank = 0;
        let degrees = names(
            &derived,
            &Target::Geographic(4326),
            &Vertical::Epsg(5720),
            &Local::identity(),
            &mut rank,
        )
        .unwrap();
        assert!(degrees.local.is_none());
        assert_eq!(rank, 2);
    }

    #[test]
    fn an_unstated_vertical_datum_is_written_as_unknown_and_not_guessed() {
        // The failure this prevents is silent: ellipsoidal height and NGF-IGN69
        // differ by some forty metres in France, and both are plausible numbers.
        // The standard has a type for *nobody said*, so nobody has to guess.
        let made = built(&Vertical::default());
        let Some(eml::AbstractVerticalCrs::VerticalUnknownCrs(unknown)) =
            &made.vertical.abstract_vertical_crs
        else {
            panic!("expected an unknown vertical CRS")
        };
        // And it says *why*, rather than carrying an empty string that reads as
        // a field somebody forgot.
        assert!(unknown.unknown.contains("vertical datum"), "{}", unknown.unknown);
        // Over metres the compound is written on it all the same: the
        // `VerticalCrs` states its direction and unit (CRS.xsd l. 705), which
        // is what the compound's axis agrees with.
        let metric = build(
            &Target::Projected(2154),
            &Vertical::default(),
            &Local::identity(),
            &named(true),
            citation,
        )
        .unwrap();
        assert!(metric.local.is_some());
    }

    #[test]
    fn the_axis_points_up_and_the_unit_is_the_metre() {
        // A depth-positive axis would put every point below the datum, and the
        // file would still be valid. RFC 7946 §3.1.1 says height, in metres.
        let made = built(&Vertical::default());
        assert_eq!(made.vertical.direction, eml::VerticalDirection::Up);
        assert_eq!(made.vertical.uom, eml::LengthUomExt::M);
    }

    #[test]
    fn the_uuids_are_the_callers_so_a_second_run_names_the_same_objects() {
        let a = metres(&Local::identity());
        let b = metres(&Local::identity());
        assert_eq!(a.horizontal.uuid(), b.horizontal.uuid());
        assert_eq!(a.vertical.uuid, b.vertical.uuid);
        // And they are distinct — one uuid for two objects would make a package
        // that cannot address one of them.
        let uuids: std::collections::BTreeSet<String> = a.new_objects().into_iter().map(|o| o.uuid).collect();
        assert_eq!(uuids.len(), 4);
    }

    #[test]
    fn a_retitled_frame_is_the_one_a_geometry_names() {
        // A caller's title names *the frame* — what a form offers as `crs` — and
        // not the halves, which keep the titles that say what they are.
        let mut made = metres(&Local::identity());
        made.retitle("Lutetia — profondeur");
        assert_eq!(made.reference().title, "Lutetia — profondeur");
        assert_eq!(made.horizontal.title(), "EPSG:2154");
        assert_eq!(made.vertical.citation.title, "EPSG:5720");
    }

    #[test]
    fn every_frame_the_list_offers_is_one_the_classifier_takes_and_no_half_is() {
        // Two statements of one fact — the list a form offers and what an
        // operation accepts — confronted, so that neither drifts alone.
        for one in FRAMES {
            assert!(frame_kind(one).is_ok(), "{one} is offered and refused");
        }
        for half in [
            "eml23.VerticalCrs",
            "eml23.LocalEngineering2dCrs",
            "eml23.ReferencePointInACrs",
            "eml23.ReferencePointInALocalEngineeringCompoundCrs",
            "resqml22.HorizonInterpretation",
            "",
        ] {
            let why = frame_kind(half).expect_err(half);
            assert!(
                half.is_empty() || why.contains(half),
                "the refusal names what it got: {why}"
            );
        }
        // By the bare class name: case, package and 2.0.1's `obj_` are spelling.
        assert_eq!(frame_kind("resqml20.obj_LocalDepth3dCrs"), Ok(FrameKind::Legacy));
        assert_eq!(
            frame_kind("eml23.localengineeringcompoundcrs"),
            Ok(FrameKind::Compound)
        );
    }
}

/// Whether this EPSG code measures its horizontal axes in **degrees**.
///
/// `None` when the code is unknown to this build: *I do not know* and *no* are
/// not the same answer, and a caller that confused them would write degrees
/// under a `ProjectedCrs`.
///
/// Read from the proj4 definition rather than from a list of codes: a list
/// would be a second truth about what `place` already does, and it would age.
pub fn is_geographic(epsg: i64) -> Option<bool> {
    energyml_proj::Proj4::new()
        .definition(u64::try_from(epsg).ok()?)
        .ok()
        .map(|definition| definition.contains("+proj=longlat"))
}

/// Turn the read positions into the coordinates the target wants.
///
/// `Geographic` passes them through — the third ordinate included, untouched,
/// because a change of horizontal system is not a change of datum for the
/// height and pretending otherwise would move every elevation.
///
/// `Projected` reprojects, and **refuses by naming the pair** rather than
/// dropping a point: a cloud silently short of a hundred places looks exactly
/// like a survey that had a hundred fewer.
pub fn place(
    points: &[crate::geojson::Position],
    target: &Target,
    from: i64,
) -> Result<Vec<[f64; 3]>, String> {
    let flat = |p: &crate::geojson::Position| [p.lon, p.lat, p.z.unwrap_or(f64::NAN)];
    let Target::Projected(to) = target else {
        return Ok(points.iter().map(flat).collect());
    };
    // **A system onto itself is not computed.** A table already projected
    // writes its own code as target; going through the datum and back would
    // return coordinates a few microns off their own, that is, numbers the
    // producer did not write, for nothing.
    if *to == from {
        return Ok(points.iter().map(flat).collect());
    }
    use energyml_domain::reproject::Reprojector;
    let backend = energyml_proj::Proj4::new();
    let mut out = Vec::with_capacity(points.len());
    for point in points {
        let placed = backend
            .reproject(flat(point), from as u64, *to as u64)
            .map_err(|e| {
                format!(
                    "[{}, {}] could not go from EPSG:{from} to EPSG:{to}: {e}",
                    point.lon, point.lat
                )
            })?;
        out.push(placed);
    }
    Ok(out)
}

#[cfg(test)]
mod projection {
    use super::*;
    use crate::geojson::Position;

    fn citation(title: &str) -> eml::Citation {
        eml::Citation {
            title: title.to_string(),
            originator: "test".to_string(),
            creation: "2026-09-08T00:00:00Z".to_string(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        }
    }

    fn paris() -> Vec<Position> {
        vec![
            Position {
                lon: 2.2953,
                lat: 48.875,
                z: Some(58.0),
            },
            Position {
                lon: 2.4199,
                lat: 48.8662,
                z: Some(85.8),
            },
        ]
    }

    #[test]
    fn degrees_pass_through_without_moving_a_bit() {
        // The meaning of a geographic target: nothing is transformed, so nothing
        // can be lost. Compared bit for bit — an identity reprojection that went
        // through the computation again would return *almost* equal numbers, and
        // "almost" is what this branch exists to avoid.
        let placed = place(&paris(), &Target::Geographic(4326), 4326).expect("no refusal");
        assert_eq!(placed[0][0].to_bits(), 2.2953_f64.to_bits());
        assert_eq!(placed[0][1].to_bits(), 48.875_f64.to_bits());
        assert_eq!(placed[0][2].to_bits(), 58.0_f64.to_bits());
    }

    #[test]
    fn projecting_gives_metres_and_the_altitude_does_not_move() {
        // Lambert-93 over Paris: easting around 650 km, northing around
        // 6 860 km. The bounds are wide — what is checked is the order of
        // magnitude and the **order of the axes**, not the value, which is the
        // business of `energyml-proj` and its own tests against PROJ.
        let placed = place(&paris(), &Target::Projected(2154), 4326).expect("no refusal");
        assert!(
            (600_000.0..700_000.0).contains(&placed[0][0]),
            "easting inattendu : {}",
            placed[0][0]
        );
        assert!(
            (6_800_000.0..6_900_000.0).contains(&placed[0][1]),
            "northing inattendu : {}",
            placed[0][1]
        );
        // **The elevation is not touched.** Changing the horizontal system is
        // not changing the vertical datum, and doing so would shift every
        // elevation by some forty metres without any check seeing
        // it.
        assert_eq!(placed[0][2], 58.0);
    }

    #[test]
    fn projecting_gives_a_scene_with_the_right_proportions() {
        // The defect that motivated all this, measured the other way round. In
        // degrees the Z is 335 times the horizontal extent; in metres it is a
        // fraction of it, which is what a cloud of borehole heads should be.
        let degrees = place(&paris(), &Target::Geographic(4326), 4326).unwrap();
        let metres = place(&paris(), &Target::Projected(2154), 4326).unwrap();
        let spread = |p: &[[f64; 3]], at: usize| {
            p.iter().map(|q| q[at]).fold(f64::MIN, f64::max)
                - p.iter().map(|q| q[at]).fold(f64::MAX, f64::min)
        };
        assert!(spread(&degrees, 2) / spread(&degrees, 0) > 100.0);
        assert!(spread(&metres, 2) / spread(&metres, 0) < 1.0);
    }

    #[test]
    fn the_utm_zone_is_deduced_from_the_coordinates_and_never_from_the_country() {
        // Paris, SIGEOL boreholes: 2.32 E / 48.97 N -> zone 31, northern
        // hemisphere. The longitude decides, not the country: a French user who
        // wants Lambert-93 replaces the suggestion, and that is why it is
        // one.
        assert_eq!(utm_epsg_of(2.32, 48.9682), Some(32_631));
        // South: the same zone, the other prefix.
        assert_eq!(utm_epsg_of(2.32, -48.9682), Some(32_731));
        // The two edges of the world, because a zone outside 1..60 would be an
        // EPSG code that does not exist.
        assert_eq!(utm_epsg_of(-180.0, 0.0), Some(32_601));
        assert_eq!(utm_epsg_of(179.99, 0.0), Some(32_660));
    }

    #[test]
    fn the_norwegian_exceptions_are_those_of_the_standard() {
        // Bergen: zone 32 is widened to the west, so 31 would be wrong --
        // metric and drawable, but not what the neighbouring data set uses.
        // Without the exception this point would fall in 32631.
        assert_eq!(utm_epsg_of(5.32, 60.39), Some(32_632));
        // Svalbard: four zones instead of seven.
        assert_eq!(utm_epsg_of(15.6, 78.2), Some(32_633));
        assert_eq!(utm_epsg_of(8.0, 78.0), Some(32_631));
        // And just below the band, the ordinary rule resumes.
        assert_eq!(utm_epsg_of(15.6, 70.0), Some(32_633));
    }

    #[test]
    fn a_coordinate_that_is_not_one_suggests_nothing() {
        // No fall back on a default zone: a suggestion drawn from a NaN would
        // be a plausible number put on nothing, and that is exactly the
        // shape of error this module exists to avoid.
        assert_eq!(utm_epsg_of(f64::NAN, 48.0), None);
        assert_eq!(utm_epsg_of(2.0, f64::NAN), None);
        assert_eq!(utm_epsg_of(2.0, 91.0), None);
    }

    #[test]
    fn an_epsg_code_the_table_does_not_know_is_refused_naming_the_pair() {
        // Name both ends, because that is what says what to do: add
        // the definition, or correct the code.
        let why = place(&paris(), &Target::Projected(999_999), 4326).expect_err("a refusal");
        assert!(why.contains("4326"), "{why}");
        assert!(why.contains("999999"), "{why}");
    }

    #[test]
    fn a_projected_crs_declares_its_unit_and_its_axis_order() {
        // What the geographic case cannot say, and which is the whole gain:
        // a reader knows the horizontal and the vertical are the same unit.
        let names = Names {
            horizontal: "00000000-0000-4000-8000-00000000000a".to_string(),
            vertical: "00000000-0000-4000-8000-00000000000b".to_string(),
            local: Some((
                "00000000-0000-4000-8000-00000000000c".to_string(),
                "00000000-0000-4000-8000-00000000000d".to_string(),
            )),
        };
        let made = build(
            &Target::Projected(2154),
            &Vertical::Epsg(5720),
            &Local::identity(),
            &names,
            citation,
        )
        .unwrap();
        let Horizontal::Projected(crs) = &made.horizontal else {
            panic!("asked for metres, got degrees")
        };
        assert_eq!(crs.uom, Some(eml::LengthUomExt::M));
        assert_eq!(crs.axis_order, eml::AxisOrder2d::EastingNorthing);
        // The geometry names the compound now — the `ProjectedCrs` is inside it,
        // embedded in the plane, not beside it.
        assert_eq!(
            made.reference().qualified_type,
            "eml23.LocalEngineeringCompoundCrs"
        );
    }
}
