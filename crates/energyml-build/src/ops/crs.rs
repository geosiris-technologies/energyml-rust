//! `crs` — a coordinate reference system from an EPSG code, out of nothing.
//!
//! The most degenerate operation, and the one that puts the trait to the test
//! from the far end: **no input at all**. A design that made the input
//! mandatory would have left this case out, and it is one that has to be
//! easy.
//!
//! # Four objects over metres, two over degrees
//!
//! EML 2.3 says the halves of a system apart, and a geometry names what joins
//! them: *« From: AbstractGeometry.LocalCrs — To: LocalEngineeringCompoundCrs »*
//! (RESQML TRG v2.2, §3.1, p. 22). So over metres this writes the
//! `ProjectedCrs`, the `VerticalCrs`, the `LocalEngineering2dCrs` set on the
//! first (embedded as its `OriginProjectedCrs`) and the
//! `LocalEngineeringCompoundCrs` that joins the plane to the vertical — and the
//! compound is the object a geometry names, role [`crate::crs::FRAME_ROLE`].
//!
//! The doc used to say that *a `LocalCrs` a representation attaches to
//! references both* halves: it did not — there was no compound, and what a
//! geometry named was the horizontal half, the vertical referenced by nothing
//! (a defect found by an audit of the reference model).
//!
//! Over **degrees** there is no compound to write: `OriginProjectedCrs` is a
//! `ProjectedCrs` (`CRS.xsd` l. 504). The two halves are written, the plan
//! says it (`frame-degrees`), and asking for an origin, an azimuth or another
//! axis on degrees is refused — a deliberate decision.
//!
//! # What the local frame can say
//!
//! By default the **identity** — origin 0/0, azimuth 0, the axis counting the
//! way the vertical system does, in metres, origin 0 in it — so what is written
//! under it is what was written under the `ProjectedCrs`. The parameters are
//! the ones the reference model's frames need, and their names are shared
//! with the recipe that builds that model:
//!
//! - `originEasting`, `originNorthing` — where the local origin sits in the
//!   projected system;
//! - `azimuth` — the rotation of the local Y axis, **degrees clockwise from
//!   grid north** (RESQML 2.0.1 `Common.xsd` l. 136–145 for the sense, the one
//!   the azimuth replaced; `AzimuthReference` written `grid north`);
//! - `axis` — `elevation` (the vertical system's own sense), `depth` (down),
//!   `time` (down, `IsTime`, in `timeUom`);
//! - `originVertical` — the vertical coordinate of the local origin **in the
//!   vertical system**, its sense and unit (`CRS.xsd` l. 517–520).
//!
//! # A code of degrees under a `ProjectedCrs` is refused
//!
//! `crs --set epsg=4326` wrote a `ProjectedCrs` whose code measures degrees,
//! and nothing said so — `crate::crs::is_geographic` existed and was not called
//! (a defect found in audit). Both mismatches are refused by name now, and a code this build
//! does not know is written as asked and said to be unchecked: *I do not know*
//! is not *no*.

use energyml_domain::provenance::Parameter;

use crate::crs::{Axis, Local, Target, Vertical};
use crate::declare::{Epsg, Params, Title};
use crate::operation::{About, ActivityDraft, Category, Context, Note, Operation, Plan, Refusal};

/// The operation.
pub struct Crs;

/// What `crs` is told — and nothing taken: `Inputs = ()`.
#[derive(Debug, Params)]
pub struct CrsParams {
    /// the EPSG code of the horizontal system
    pub epsg: Epsg,
    /// metres under a ProjectedCrs and a local compound (true), or degrees under a Geographic2dCrs (false)
    #[param(default = true, optional)]
    pub projected: bool,
    /// the EPSG code of the vertical system; left out, the package says unknown
    pub vertical: Option<Epsg>,
    /// the easting of the local origin, in the projected system
    #[param(default = 0.0, optional, uom = "m")]
    pub origin_easting: f64,
    /// the northing of the local origin, in the projected system
    #[param(default = 0.0, optional, uom = "m")]
    pub origin_northing: f64,
    /// the rotation of the local Y axis, clockwise from grid north
    #[param(default = 0.0, optional, uom = "dega")]
    pub azimuth: f64,
    /// Which way the local z counts.
    #[param(flatten)]
    pub axis: AxisParams,
    /// the vertical coordinate of the local origin, in the vertical system — its sense and its unit
    #[param(default = 0.0, optional, uom = "m")]
    pub origin_vertical: f64,
    /// the title of the frame a geometry names — the compound, or the horizontal system in degrees; defaults to what it is
    pub title: Option<Title>,
}

/// the local vertical axis: elevation counts the way the vertical system does, depth counts down, time counts down in a time unit
#[derive(Debug, Params)]
#[params(tag = "axis")]
pub enum AxisParams {
    /// The vertical system's own sense, in metres.
    #[case(default)]
    Elevation,
    /// Down, in metres.
    Depth,
    /// Down, in a time unit.
    Time(TimeAxis),
}

/// A time axis.
#[derive(Debug, Params)]
pub struct TimeAxis {
    /// time: the unit the axis counts in — ms, s
    #[param(default = String::from("ms"), optional)]
    pub time_uom: String,
}

impl AxisParams {
    fn axis(&self) -> Axis {
        match self {
            AxisParams::Elevation => Axis::Elevation,
            AxisParams::Depth => Axis::Depth,
            AxisParams::Time(time) => Axis::Time(time.time_uom.trim().to_string()),
        }
    }
}

impl Operation for Crs {
    type Params = CrsParams;
    type Inputs = ();

    fn about(&self) -> About {
        About {
            name: "crs",
            describe: "a coordinate system from EPSG codes — the projected and vertical halves and the local compound a geometry names, with an origin, an azimuth and a depth or time axis on demand — takes no input",
            category: Category::Frame,
            makes: &[
                "eml23.ProjectedCrs",
                "eml23.Geographic2dCrs",
                "eml23.VerticalCrs",
                "eml23.LocalEngineering2dCrs",
                "eml23.LocalEngineeringCompoundCrs",
            ],
            recipe: "crs",
            represents: None,
        }
    }

    fn plan(&self, _inputs: (), params: CrsParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let epsg = params.epsg.0;
        let mut says = Vec::new();
        // **A code's unit is checked against the form asked for.** Degrees
        // under a `ProjectedCrs` are read as metres by every reader, and the
        // model lands a hundred thousand times too small; metres under a
        // `Geographic2dCrs` are read as degrees, and it lands off the planet.
        match (crate::crs::is_geographic(epsg), params.projected) {
            (Some(true), true) => {
                return Err(Refusal::on(
                    "epsg",
                    format!("EPSG:{epsg} measures degrees, and `projected` asks for a ProjectedCrs, whose coordinates are metres: set `projected` to false to keep the degrees under a Geographic2dCrs, or give a projected code (EPSG:2154 for mainland France)"),
                ));
            }
            (Some(false), false) => {
                return Err(Refusal::on(
                    "epsg",
                    format!("EPSG:{epsg} is a projected system, in metres, and `projected` false asks for a Geographic2dCrs, whose coordinates are degrees: leave `projected` true, or give a geographic code (EPSG:4326)"),
                ));
            }
            (None, _) => says.push(Note {
                key: "epsg-unchecked",
                detail: format!("EPSG:{epsg} is not in this build's table, so whether it measures degrees or metres was not checked — it is written as asked"),
            }),
            _ => {}
        }
        let target = if params.projected {
            Target::Projected(epsg)
        } else {
            Target::Geographic(epsg)
        };
        let vertical = match params.vertical {
            Some(code) => Vertical::Epsg(code.0),
            None => Vertical::Unknown("no vertical datum was given".to_string()),
        };
        let vertical_key = vertical.key();
        let local = Local {
            origin: [params.origin_easting, params.origin_northing],
            azimuth: params.azimuth,
            axis: params.axis.axis(),
            origin_vertical: params.origin_vertical,
        };
        let mut rank = 0usize;
        let names = crate::crs::names(cx.identity, &target, &vertical, &local, &mut rank)?;
        let citation = |title: &str| energyml_eml_2_3::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        };
        let mut built = crate::crs::build(&target, &vertical, &local, &names, citation).map_err(|why| {
            // Every refusal of `build` is about what this operation was told —
            // a number, the time unit, a local frame over degrees — and it
            // names the setting in backticks when it is one.
            let slot = [
                "timeUom",
                "originEasting",
                "originNorthing",
                "azimuth",
                "originVertical",
            ]
            .into_iter()
            .find(|name| why.contains(&format!("`{name}`")))
            .unwrap_or(if matches!(target, Target::Geographic(_)) {
                "projected"
            } else {
                "axis"
            });
            Refusal::on(slot, why)
        })?;
        if let Some(title) = params.title {
            built.retitle(&title.0);
        }
        if built.local.is_none() {
            says.push(Note {
                key: "frame-degrees",
                detail: format!(
                    "EPSG:{epsg} is written as a Geographic2dCrs, in degrees: EML 2.3 has no compound over degrees (LocalEngineering2dCrs.OriginProjectedCrs is a ProjectedCrs, CRS.xsd l. 504), so a geometry names the Geographic2dCrs and the vertical system is referenced by nothing"
                ),
            });
        }
        let frame = built.reference();

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: built.new_objects(),
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "crs",
                title: format!("crs → {}", frame.title),
                // What tells two runs apart is what tells their compounds
                // apart — the frame's uuid carries all of it.
                seed: format!("{epsg}:{vertical_key}:{}", frame.uuid),
                parameters: vec![
                    Parameter::Double {
                        title: "epsg".to_string(),
                        value: epsg as f64,
                        uom: None,
                    },
                    Parameter::Text {
                        title: "vertical".to_string(),
                        value: vertical_key,
                    },
                ],
            },
            says,
            dropped: Vec::new(),
            // **No shape**: this operation computes no geometry.
            // An empty list that is an answer, not an oversight.
            shapes: Vec::new(),
        })
    }
}
