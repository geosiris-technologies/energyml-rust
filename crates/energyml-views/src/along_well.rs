//! `along-well`: the geosteering section, a vertical section along a well.
//!
//! This is a wrapper of [`energyml_section::along_well`], which holds everything: [`section::offers`] says what a
//! frame offers, [`section::build`] reads and places the hole, the curtain, the boundaries, the horizons and the
//! tops, [`section::layout`] lays them out. This module declares the settings (the fields of the section's
//! [`section::Request`] and [`section::View`], flattened; a structure that does not flatten is a JSON setting),
//! fetches the placed surfaces through [`section::PlacedMesh::read`] (the one door of the geometry), colours the
//! curtain through [`crate::colour::value_colours`], and calls `layout`. It does not call
//! `energyml_section::along_well::draw`: the metadata of a picture is the one of this crate (the [`ViewAsk`]).
//!
//! [`ViewAsk`]: crate::ViewAsk

use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_domain::palettes::Reading;
use energyml_domain::stats::scale::ScaleKind;
use energyml_geometry::around_well::projection::WellProjections;
use energyml_section::along_well as section;
use serde_json::Value;

use crate::colour::value_colours;
use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::{Settled, ViewSetting};

/// The id of the view.
pub const ID: &str = "along-well";

/// The types of the frame it hangs on.
pub const FRAME_TYPES: &[&str] = &["WellboreFrameRepresentation"];

/// The types of the trajectory, when one is given instead of the frame's own.
pub const TRAJECTORY_TYPES: &[&str] = &["WellboreTrajectoryRepresentation"];

/// The types of the curtain's property.
pub const PROPERTY_TYPES: &[&str] = &["ContinuousProperty"];

/// The types of the surfaces whose horizons are cut.
pub const SURFACE_TYPES: &[&str] = &["TriangulatedSetRepresentation", "Grid2dRepresentation"];

/// The projections it offers (the ids of `WellProjections`, and `vertical_section`), the first the default.
pub const PROJECTIONS: &[&str] = &["md_tvd", "developed", "vertical_section"];

/// Which axis of the plane a dimension of the curtain runs along.
pub const AXES: &[&str] = &["high_side", "right"];

/// The sign of a dimension of the curtain.
pub const SIGNS: &[&str] = &["positive", "negative"];

/// The scale of the values of the curtain: proposed from the decades they span, or fixed.
pub const SCALES: &[&str] = &["auto", "linear", "log10"];

/// The curtain source asked when none is named.
pub const DEFAULT_SOURCE: &str = "cube-on-frame";

/// The view.
pub struct AlongWell;

impl ViewKind for AlongWell {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "a vertical section along a well (MD against TVD, developed or vertical section) with the curtain of a cube either side of the hole, the boundaries of a depth of detection, the horizons the curtain cuts and the tops of the hole"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![
            ObjectSlot {
                role: "frame",
                types: FRAME_TYPES,
                least: 1,
                most: 1,
                about: "the wellbore frame the curtain hangs on (its nodes are the columns)",
            },
            ObjectSlot {
                role: "trajectory",
                types: TRAJECTORY_TYPES,
                least: 0,
                most: 1,
                about: "the trajectory to place the hole with; none: the one the frame references",
            },
            ObjectSlot {
                role: "property",
                types: PROPERTY_TYPES,
                least: 0,
                most: 1,
                about: "the property of the curtain: a matrix of two dimensions per node of the frame; none: no curtain",
            },
            ObjectSlot {
                role: "surface",
                types: SURFACE_TYPES,
                least: 0,
                most: 64,
                about: "a surface whose horizon the curtain cuts, and whose tops along the hole are computed when asked",
            },
        ]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        let shown = |name: &'static str, about: &'static str| {
            ViewSetting::flag(name, about).default(Value::Bool(true))
        };
        vec![
            // The fields of `Request`.
            ViewSetting::choice("projection", PROJECTIONS, "how the hole is laid flat along the abscissa")
                .default(Value::from(PROJECTIONS[0])),
            ViewSetting::number(
                "azimuth",
                "for `vertical_section`: the azimuth of the section plane, degrees clockwise from grid north",
            )
            .uom("dega")
            .bounds(Some(0.0), Some(360.0)),
            ViewSetting::json(
                "origin",
                "for `vertical_section`: the point the section starts from, `[east, north]` in metres, absolute",
            ),
            ViewSetting::number(
                "step",
                "the spacing of the samples along the hole, in metres; none: the median node step clamped to 0.1 to 5",
            )
            .bounds(Some(0.001), None),
            ViewSetting::json(
                "md_range",
                "the measured depths shown, `[from, to]`; none: the extent of the frame",
            ),
            ViewSetting::json(
                "dn_range",
                "the offsets along the high side the horizons are looked for in, `[low, high]` in metres; none: the curtain's rows, else the boundaries' largest radius",
            ),
            ViewSetting::flag(
                "computed_tops",
                "compute the tops of the hole itself where it crosses each surface",
            ),
            ViewSetting::text(
                "curtain_source",
                "the source of the curtain's columns: `cube-on-frame`, the one built",
            )
            .default(Value::from(DEFAULT_SOURCE)),
            ViewSetting::json(
                "curtain_links",
                "the axes of the curtain, declared: a list of `{\"kind\": \"table\" | \"paired-property\" | \"witsml-channel\", ...}` as the section reads them, tried before the file's own convention",
            ),
            ViewSetting::choice(
                "slow_axis",
                AXES,
                "the axis of the plane the slow dimension of the curtain runs along (required with a curtain: the file does not state it)",
            ),
            ViewSetting::choice(
                "slow_sign",
                SIGNS,
                "whether the offsets of the slow dimension count along the axis or against it (required with a curtain)",
            ),
            ViewSetting::choice(
                "fast_axis",
                AXES,
                "the axis of the plane the fast dimension of the curtain runs along (required with a curtain)",
            ),
            ViewSetting::choice(
                "fast_sign",
                SIGNS,
                "whether the offsets of the fast dimension count along the axis or against it (required with a curtain)",
            ),
            ViewSetting::integer(
                "lateral_index",
                "the row shown along the lateral dimension of the curtain",
            )
            .bounds(Some(0.0), None)
            .default(Value::from(0)),
            ViewSetting::json(
                "boundaries",
                "the boundaries of a depth of detection: `{\"radius\": uri, \"angle\": uri, \"mode\": \"in-plane\" | \"projected\"}`, the two properties full uris of the package",
            ),
            // The fields of `View`.
            ViewSetting::json(
                "window",
                "the part of the drawing shown, `[x0, tvd0, x1, tvd1]` in drawing units; none: everything",
            ),
            ViewSetting::number(
                "exaggeration",
                "the vertical scale over the horizontal one; none: what the window and the size give",
            )
            .bounds(Some(0.001), Some(1000.0)),
            shown("show_curtain", "draw the curtain"),
            shown("show_boundaries", "draw the boundaries"),
            shown("show_surfaces", "draw the horizons"),
            shown("show_tops", "draw the tops of the hole"),
            ViewSetting::flag("legend", "draw the legend of the curtain's values"),
            ViewSetting::choice(
                "scale",
                SCALES,
                "the scale of the curtain's values: `auto` proposes a log10 scale from three decades",
            )
            .default(Value::from(SCALES[0])),
            ViewSetting::number("min", "the lower bound of the curtain's values; none: the first percentile"),
            ViewSetting::number("max", "the upper bound of the curtain's values; none: the ninety-ninth percentile"),
            ViewSetting::text(
                "palette",
                "the palette of the curtain's values, by name; none: the file's colour map, else the default palette",
            ),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [900.0, 600.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        let mut out = Vec::new();
        for node in index.nodes().filter(|node| {
            node.state == NodeState::Present && FRAME_TYPES.contains(&local_type(&node.qualified_type))
        }) {
            let frame = node.uri.to_string();
            let name = node.title.as_deref().unwrap_or(&node.uuid).to_string();
            out.push(Offer {
                view: ID,
                objects: vec![frame.clone()],
                title: format!("{name}: the hole"),
                missing: Vec::new(),
            });
            for (property, needs) in section::offers(index, &frame).curtains {
                let property_name = index
                    .node(&property)
                    .and_then(|found| found.title.as_deref())
                    .unwrap_or(&property)
                    .to_string();
                let mut missing = vec![
                    "the roles and signs of the curtain's two dimensions (slow_axis, slow_sign, fast_axis, fast_sign): the file does not state them".to_string(),
                ];
                missing.extend(needs.iter().map(need_in_words));
                out.push(Offer {
                    view: ID,
                    objects: vec![frame.clone(), property],
                    title: format!("{name}: the curtain {property_name}"),
                    missing,
                });
            }
        }
        out
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let settings = &request.settings;
        let frame = request
            .object("frame")
            .ok_or_else(|| refuse("no frame given".to_string()))?;
        let projection = settings.text("projection").unwrap_or(PROJECTIONS[0]);

        // The vertical section: its azimuth and its origin are the caller's, never guessed.
        let vertical = if projection == "vertical_section" {
            let azimuth = settings
                .number("azimuth")
                .ok_or_else(|| ViewRefusal::MissingSetting {
                    view: ID.to_string(),
                    name: "azimuth".to_string(),
                    about: "a vertical section needs the azimuth of its plane".to_string(),
                })?;
            let origin: [f64; 2] =
                parsed(settings, "origin")?.ok_or_else(|| ViewRefusal::MissingSetting {
                    view: ID.to_string(),
                    name: "origin".to_string(),
                    about: "a vertical section needs the point it starts from, [east, north] in metres"
                        .to_string(),
                })?;
            Some(section::SectionAsk {
                azimuth: azimuth.to_radians(),
                origin,
            })
        } else {
            None
        };

        // The curtain: the property is an object, its axes and roles settings.
        let curtain = match request.object("property") {
            None => None,
            Some(property) => {
                let role = |name: &'static str, about: &'static str| {
                    settings.text(name).ok_or_else(|| ViewRefusal::MissingSetting {
                        view: ID.to_string(),
                        name: name.to_string(),
                        about: about.to_string(),
                    })
                };
                let slow_axis = role(
                    "slow_axis",
                    "the axis the slow dimension of the curtain runs along",
                )?;
                let slow_sign = role("slow_sign", "the sign of the slow dimension of the curtain")?;
                let fast_axis = role(
                    "fast_axis",
                    "the axis the fast dimension of the curtain runs along",
                )?;
                let fast_sign = role("fast_sign", "the sign of the fast dimension of the curtain")?;
                Some(section::CurtainAsk {
                    property: property.to_string(),
                    source: settings
                        .text("curtain_source")
                        .unwrap_or(DEFAULT_SOURCE)
                        .to_string(),
                    links: parsed(settings, "curtain_links")?.unwrap_or_default(),
                    roles: [
                        (axis_of(slow_axis), sign_of(slow_sign)),
                        (axis_of(fast_axis), sign_of(fast_sign)),
                    ],
                    lateral_index: settings.integer("lateral_index").unwrap_or(0).max(0) as usize,
                })
            }
        };

        let surface_uris: Vec<String> = request
            .objects_of("surface")
            .into_iter()
            .map(str::to_string)
            .collect();
        let wanted = section::Request {
            frame: frame.to_string(),
            trajectory: request.object("trajectory").map(str::to_string),
            curtain,
            boundaries: parsed(settings, "boundaries")?,
            surfaces: surface_uris.clone(),
            computed_tops: settings.flag("computed_tops"),
            markers: Vec::new(),
            md_range: parsed(settings, "md_range")?,
            dn_range: parsed(settings, "dn_range")?,
            projection: projection.to_string(),
            section: vertical,
            step: settings.number("step"),
        };

        // The surfaces, placed by the one door of the geometry.
        let mut meshes = Vec::new();
        for uri in &surface_uris {
            meshes.push(section::PlacedMesh::read(cx.source, uri).map_err(|why| refuse(why.to_string()))?);
        }

        let sources = section::CurtainSources::builtin();
        let projections = WellProjections::standard();
        let model = section::build(&wanted, cx.source, &meshes, &sources, &projections)
            .map_err(|why| refuse(why.to_string()))?;

        // The colours of the curtain: the file's map, else a palette, said so, over the scale the section's legend
        // decides (`curtain_scale`, the one decision).
        let legend_ask = section::LegendAsk {
            kind: match settings.text("scale") {
                Some("linear") => Some(section::ScaleKindAsk::Linear),
                Some("log10") => Some(section::ScaleKindAsk::Log10),
                _ => None,
            },
            min: settings.number("min"),
            max: settings.number("max"),
        };
        let mut notes = Vec::new();
        let mut colours = None;
        if let (Some(curtain), Some(scale)) = (
            &model.curtain,
            section::curtain_scale(&model, &legend_ask).map_err(|why| refuse(why.to_string()))?,
        ) {
            let logarithmic = scale.kind() == ScaleKind::Log10;
            let bounds = (scale.min(), scale.max());
            let painted = value_colours(
                cx,
                ID,
                Some(&curtain.columns.property),
                settings.text("palette"),
                bounds,
                logarithmic,
                Reading::Continuous,
            )?;
            notes.push(ViewNote {
                key: "curtain-colours",
                says: painted.source.says(),
            });
            colours = Some(painted);
        }

        let view = section::View {
            size: request.size,
            window: parsed(settings, "window")?,
            exaggeration: settings.number("exaggeration"),
            show_curtain: settings.flag("show_curtain"),
            show_boundaries: settings.flag("show_boundaries"),
            show_surfaces: settings.flag("show_surfaces"),
            show_tops: settings.flag("show_tops"),
            colours: Default::default(),
            legend: settings.flag("legend").then_some(legend_ask),
            embed_curtain: true,
        };

        // The curtain goes into the SVG as an image, written in every build.
        let sample = colours
            .as_ref()
            .map(|painted| move |value: f64| painted.sample(value));
        let colour = sample.as_ref().map(|f| f as &dyn Fn(f64) -> Option<[u8; 4]>);
        let mut list =
            section::layout(&model, &view, &projections, colour).map_err(|why| refuse(why.to_string()))?;
        list.title = format!("{ID} - {projection}");
        Ok(Drawn { list, notes })
    }
}

/// A JSON setting read as the type the section takes; none when the setting is absent.
fn parsed<T: serde::de::DeserializeOwned>(settings: &Settled, name: &str) -> Result<Option<T>, ViewRefusal> {
    match settings.get(name) {
        None => Ok(None),
        Some(value) => {
            serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|why| ViewRefusal::BadSetting {
                    name: name.to_string(),
                    why: why.to_string(),
                })
        }
    }
}

/// The axis a word of [`AXES`] names (the setting checked the word).
fn axis_of(word: &str) -> section::PlaneAxisAsk {
    match word {
        "right" => section::PlaneAxisAsk::Right,
        _ => section::PlaneAxisAsk::HighSide,
    }
}

/// The sign a word of [`SIGNS`] names (the setting checked the word).
fn sign_of(word: &str) -> section::AxisSignAsk {
    match word {
        "negative" => section::AxisSignAsk::Negative,
        _ => section::AxisSignAsk::Positive,
    }
}

/// What an offer must still be told, in words.
fn need_in_words(need: &section::CandidateNeedAsk) -> String {
    match need {
        section::CandidateNeedAsk::Axes => {
            "the axes of the curtain (curtain_links): the file gives none".to_string()
        }
        section::CandidateNeedAsk::AngleConvention => {
            "the origin and the sense of the angles: the file never says".to_string()
        }
        section::CandidateNeedAsk::ChooseAngles { candidates } => {
            format!("which angles pair with it: {}", candidates.join(", "))
        }
    }
}
