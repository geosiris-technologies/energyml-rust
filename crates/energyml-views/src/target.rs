//! `target`: the hole seen end on at one measured depth, the values of a property laid in rings and sectors.
//!
//! One ring per property: a property on the nodes of a wellbore frame with `n` values per node, one per sector
//! around the hole, makes a ring of `n` coloured sectors; several properties make concentric rings, the first
//! innermost. The MD asked falls between two nodes ([`bracket`]); the values at that MD are what
//! [`interpolate_row`] gives under a method of the registry ([`Interpolations`], the default of
//! [`default_interpolation`]); the sectors are bounded by [`sector_bounds`] (or [`regular_sectors`] when the
//! caller declares the sectors even); the direction of each is [`plane_direction`] under the angle convention the
//! caller declares. The file never states that convention: none declared is [`PolarRefusal::ConventionUndeclared`],
//! passed on as it is, never a guess. The high side and the north come from [`frames_along`] and
//! [`north_in_plane`]. This module only lays them out.

use energyml_core::value::FieldValue;
use energyml_domain::along_md::{
    Bracket, GapRule, InterpNote, Interpolations, Nature, SNAP_FRACTION, bracket, default_interpolation,
    interpolate_row, median_step, nearest_node,
};
use energyml_domain::component_axes::{AxisLink, AxisSources, AxisSubject, AxisValues};
use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_domain::palettes::Reading;
use energyml_domain::series::matrix::{RowSupport, read_matrix};
use energyml_domain::units::{self, Dimension};
use energyml_domain::{property, strata_by_md};
use energyml_geometry::around_well::hole_frame::{FrameOptions, HoleFrame, frames_along, north_in_plane};
use energyml_geometry::around_well::polar::{
    AngleConvention, AngleOrigin, AngleSense, PolarRefusal, ViewedFrom, plane_direction, polar_points,
    regular_sectors, sector_bounds,
};
use energyml_geometry::well::PlacedPath;
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect, Stroke, SymbolKind,
};
use serde_json::Value;

use crate::colour::{ValueColours, value_colours};
use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::{Settled, ViewSetting};
use crate::style;

/// The id of the view.
pub const ID: &str = "target";

/// The properties it shows, one ring each.
pub const TYPES: &[&str] = &["ContinuousProperty", "DiscreteProperty"];

/// Where angle zero points, as a setting says it.
pub const ORIGINS: &[&str] = &["high_side", "low_side", "right", "left", "north"];

/// Which way angles grow.
pub const SENSES: &[&str] = &["clockwise", "anticlockwise"];

/// From where the sense is seen.
pub const VIEWED_FROM: &[&str] = &["looking_downhole", "looking_uphole"];

/// The scales of the colours.
pub const SCALES: &[&str] = &["linear", "log"];

/// The most rings one picture holds.
pub const MOST_RINGS: usize = 8;

/// How many steps the colour bar of the legend has.
const BAR_STEPS: usize = 24;

/// The view.
pub struct Target;

impl ViewKind for Target {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "the hole seen end on at one measured depth: the values of a property in rings and sectors around it, under an angle convention the caller declares"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "property",
            types: TYPES,
            least: 1,
            most: MOST_RINGS,
            about: "a property on the nodes of a wellbore frame with one value per sector; one ring each, the first innermost",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::number("md", "the measured depth shown, in the frame's MD unit").required(),
            ViewSetting::text(
                "interpolation",
                "the method between two nodes, an id of the registry (nearest, previous, linear, log_linear, angular); none: the default of the property's nature",
            ),
            ViewSetting::choice(
                "origin",
                ORIGINS,
                "where angle zero points: the file does not say, so it is declared (with `sense` and `viewed_from`)",
            ),
            ViewSetting::choice("sense", SENSES, "which way the angles grow, as seen from `viewed_from`"),
            ViewSetting::choice(
                "viewed_from",
                VIEWED_FROM,
                "from where the sense is seen: looking down the hole, or back up it",
            ),
            ViewSetting::number("offset", "added to every angle before it is placed")
                .uom("dega")
                .bounds(Some(-360.0), Some(360.0))
                .default(Value::from(0.0)),
            ViewSetting::text(
                "angles",
                "the full uri of the property whose values are the angles of the sectors, one per sector, the same at every node",
            ),
            ViewSetting::flag(
                "regular_sectors",
                "declare the sectors evenly spread, the first centred on angle zero, when the file gives no angles",
            ),
            ViewSetting::choice("scale", SCALES, "the scale of the colours").default(Value::from(SCALES[0])),
            ViewSetting::number("min", "the value at the bottom of the colour scale; none: the least drawn"),
            ViewSetting::number("max", "the value at the top of the colour scale; none: the greatest drawn"),
            ViewSetting::text("palette", "a palette by name, over the file's colour map; none: the file's map"),
            ViewSetting::flag("rings", "draw the circles that bound the rings").default(Value::Bool(true)),
            ViewSetting::flag("north", "draw the arrow of grid north across the hole").default(Value::Bool(true)),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [640.0, 760.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        index
            .nodes()
            .filter(|node| {
                node.state == NodeState::Present && TYPES.contains(&local_type(&node.qualified_type))
            })
            .filter(|node| {
                index.sheet(&node.uri).is_some_and(|sheet| {
                    sheet.lookup.is_none()
                        && sheet.value_count_per_element.len() == 1
                        && sheet.value_count_per_element[0] >= 3
                        && matches!(
                            sheet.indexable,
                            energyml_domain::property::IndexableElement::Nodes
                        )
                })
            })
            .map(|node| Offer {
                view: ID,
                objects: vec![node.uri.to_string()],
                title: node.title.as_deref().unwrap_or(&node.uuid).to_string(),
                missing: vec![
                    "the angle convention (`origin`, `sense`, `viewed_from`): the file does not state it".to_string(),
                    "the measured depth (`md`)".to_string(),
                ],
            })
            .collect()
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let settings = &request.settings;
        let uris = request.objects_of("property");
        let Some(first) = uris.first().copied() else {
            return Err(refuse("no property given".to_string()));
        };
        let Some(md) = settings.number("md") else {
            return Err(ViewRefusal::MissingSetting {
                view: ID.to_string(),
                name: "md".to_string(),
                about: "the measured depth shown".to_string(),
            });
        };
        let mut notes = Vec::new();

        // The convention is the caller's: none declared is refused by the door of the placement, by its words.
        let convention = declared_convention(settings)?;
        if convention.is_none() {
            // The door of the placement says it, by its own words.
            if let Err(why) = polar_points(&[], &[], None, None) {
                return Err(refuse(why.to_string()));
            }
        }

        // The frame the first property hangs on, its nodes, and where the MD falls among them.
        let (frame_uri, frame) = frame_of(cx, first)?;
        let mds = strata_by_md::node_mds(frame.as_reflect(), cx.source).map_err(|why| refuse(why.to_string()))?;
        let tolerance = median_step(&mds).map(|step| step * SNAP_FRACTION).unwrap_or(0.0);
        let placed = bracket(&mds, md, tolerance).map_err(|why| refuse(why.to_string()))?;
        let rule = GapRule::of_axis(&mds).map_err(|why| refuse(why.to_string()))?;
        let support = Support {
            gap: mds[placed.upper] - mds[placed.lower],
            mds,
            placed,
            rule,
        };

        // The hole at that MD: the high side and the north. Its absence is said, and refuses only a north origin.
        let hole = hole_at(cx, frame.as_reflect(), md);
        if let Err(why) = &hole.frame {
            notes.push(ViewNote {
                key: "hole-frame-unavailable",
                says: format!(
                    "the frame of the hole at MD {} is not known ({why}): the high side and the north are not marked",
                    short(md)
                ),
            });
        } else if let Some(why) = &hole.north_why {
            notes.push(ViewNote {
                key: "north-unavailable",
                says: format!("grid north across the hole at MD {} is not known ({why})", short(md)),
            });
        }

        // One ring per property.
        let registry = Interpolations::builtin();
        let angles_uri = settings.text("angles");
        if let Some(uri) = angles_uri
            && cx.index.node(uri).is_none()
        {
            return Err(ViewRefusal::NotInPackage { object: uri.to_string() });
        }
        let mut read = Vec::new();
        for uri in &uris {
            read.push(read_ring(
                cx,
                request,
                uri,
                &frame_uri,
                &support,
                &registry,
                angles_uri,
                convention.as_ref(),
                hole.north,
                &mut notes,
            )?);
        }

        // The colours of each ring, over the range asked or the range drawn.
        let logarithmic = settings.text("scale") == Some("log");
        let mut rings = Vec::new();
        for data in read {
            let finite: Vec<f64> = data.values.iter().copied().filter(|v| v.is_finite()).collect();
            if finite.is_empty() {
                return Err(refuse(format!(
                    "`{}` has no value at MD {}{}",
                    data.title,
                    short(md),
                    data.first_refusal
                        .as_deref()
                        .map(|why| format!(": {why}"))
                        .unwrap_or_default()
                )));
            }
            let drawn_min = finite.iter().copied().fold(f64::INFINITY, f64::min);
            let drawn_max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mut low = settings.number("min").unwrap_or(drawn_min);
            let mut high = settings.number("max").unwrap_or(drawn_max);
            if low > high {
                return Err(ViewRefusal::BadSetting {
                    name: "min".to_string(),
                    why: format!("{} is above `max` ({})", short(low), short(high)),
                });
            }
            if low == high {
                let pad = if low == 0.0 { 1.0 } else { low.abs() * 0.05 };
                low -= pad;
                high += pad;
            }
            if logarithmic && low <= 0.0 {
                return Err(refuse(format!(
                    "a logarithmic scale needs a positive range, and `{}` runs from {}: give `min` above zero",
                    data.title,
                    short(low)
                )));
            }
            let colours = value_colours(
                cx,
                ID,
                Some(data.uri.as_str()),
                settings.text("palette"),
                (low, high),
                logarithmic,
                Reading::Continuous,
            )?;
            rings.push(Ring {
                data,
                colours,
                range: (drawn_min, drawn_max),
                scale: (low, high),
            });
        }

        let blank = rings.iter().map(|ring| ring.data.values.iter().filter(|v| !v.is_finite()).count()).sum::<usize>();
        if blank > 0 {
            let why = rings
                .iter()
                .find_map(|ring| ring.data.first_refusal.clone())
                .map(|why| format!(": {why}"))
                .unwrap_or_default();
            notes.push(ViewNote {
                key: "cells-without-value",
                says: format!("{blank} sector(s) have no value at MD {} and are left empty{why}", short(md)),
            });
        }

        let md_uom = hole.md_uom.clone();
        let list = picture(
            request.size,
            md,
            &md_uom,
            &support,
            &rings,
            convention.as_ref(),
            &hole,
            settings,
            cx.index
                .node(first)
                .and_then(|node| node.title.as_deref())
                .unwrap_or(first),
        )?;
        Ok(Drawn { list, notes })
    }
}

/// Where the MD asked falls among the nodes.
struct Support {
    mds: Vec<f64>,
    placed: Bracket,
    rule: GapRule,
    gap: f64,
}

/// What one property gives at the MD asked.
struct RingData {
    uri: String,
    title: String,
    uom: Option<String>,
    /// One value per sector; `NaN` where absent.
    values: Vec<f64>,
    /// The bounds of each sector in the file's angles, radians.
    bounds: Vec<[f64; 2]>,
    /// The method that gave the values between two nodes, and whether it was used.
    method: &'static str,
    first_refusal: Option<String>,
}

/// A ring ready to draw.
struct Ring {
    data: RingData,
    colours: ValueColours,
    /// The least and the greatest value drawn.
    range: (f64, f64),
    /// The bounds of the colour scale.
    scale: (f64, f64),
}

/// What is known of the hole at the MD asked.
struct Hole {
    md_uom: String,
    frame: Result<HoleFrame, String>,
    /// Grid north in the plane across the hole, `(right, high side)`.
    north: Option<[f64; 2]>,
    north_why: Option<String>,
}

/// The convention the settings declare: all three words, or none.
fn declared_convention(settings: &Settled) -> Result<Option<AngleConvention>, ViewRefusal> {
    let words = (settings.text("origin"), settings.text("sense"), settings.text("viewed_from"));
    if words.0.is_none() && words.1.is_none() && words.2.is_none() {
        return Ok(None);
    }
    let need = |name: &str| ViewRefusal::MissingSetting {
        view: ID.to_string(),
        name: name.to_string(),
        about: "an angle convention is declared whole: `origin`, `sense` and `viewed_from` together".to_string(),
    };
    let origin = match words.0 {
        Some("high_side") => AngleOrigin::HighSide,
        Some("low_side") => AngleOrigin::LowSide,
        Some("right") => AngleOrigin::Right,
        Some("left") => AngleOrigin::Left,
        Some("north") => AngleOrigin::North,
        Some(other) => {
            return Err(ViewRefusal::BadSetting {
                name: "origin".to_string(),
                why: format!("`{other}` is not one of {}", ORIGINS.join(", ")),
            });
        }
        None => return Err(need("origin")),
    };
    let sense = match words.1 {
        Some("clockwise") => AngleSense::Clockwise,
        Some("anticlockwise") => AngleSense::Anticlockwise,
        Some(other) => {
            return Err(ViewRefusal::BadSetting {
                name: "sense".to_string(),
                why: format!("`{other}` is not one of {}", SENSES.join(", ")),
            });
        }
        None => return Err(need("sense")),
    };
    let viewed_from = match words.2 {
        Some("looking_downhole") => ViewedFrom::LookingDownhole,
        Some("looking_uphole") => ViewedFrom::LookingUphole,
        Some(other) => {
            return Err(ViewRefusal::BadSetting {
                name: "viewed_from".to_string(),
                why: format!("`{other}` is not one of {}", VIEWED_FROM.join(", ")),
            });
        }
        None => return Err(need("viewed_from")),
    };
    Ok(Some(AngleConvention {
        origin,
        sense,
        viewed_from,
        offset: settings.number("offset").unwrap_or(0.0).to_radians(),
    }))
}

/// The frame a property hangs on: its full uri and the object.
fn frame_of(
    cx: &ViewContext<'_>,
    property_uri: &str,
) -> Result<(String, Box<dyn energyml_core::registry::DynObject>), ViewRefusal> {
    let refuse = |why: String| ViewRefusal::computing(ID, why);
    let object = cx.source.read_object_at(property_uri).map_err(refuse)?;
    let read = property::read(object.as_reflect()).map_err(|why| refuse(why.to_string()))?;
    let uri = read
        .supporting_representation_uri
        .clone()
        .or_else(|| read.supporting_representation.clone())
        .ok_or_else(|| refuse(format!("`{property_uri}` names no supporting representation")))?;
    let frame = cx.source.read_object_at(&uri).map_err(refuse)?;
    if local_type(frame.as_reflect().type_info().name) != "WellboreFrameRepresentation" {
        return Err(refuse(format!(
            "`{property_uri}` hangs on `{uri}`, which is not a wellbore frame: a target is read at the MD of a frame's nodes"
        )));
    }
    Ok((uri, frame))
}

/// The hole at `md`: the frame, the north, and the unit of the MD.
fn hole_at(cx: &ViewContext<'_>, frame: &dyn energyml_core::value::Reflect, md: f64) -> Hole {
    let mut hole = Hole {
        md_uom: String::new(),
        frame: Err("the frame names no trajectory".to_string()),
        north: None,
        north_why: None,
    };
    let Some(FieldValue::Object(reference)) = frame.field("Trajectory") else {
        return hole;
    };
    let trajectory = match cx.source.read_object_of(reference) {
        Ok(object) => object,
        Err(why) => {
            hole.frame = Err(why);
            return hole;
        }
    };
    let path = match PlacedPath::of(trajectory.as_reflect(), cx.source) {
        Ok(path) => path,
        Err(why) => {
            hole.frame = Err(why.to_string());
            return hole;
        }
    };
    hole.md_uom = path.md_uom().to_string();
    hole.frame = match frames_along(&path, &[md], &FrameOptions::default()) {
        Ok(frames) => frames.frames.into_iter().next().unwrap_or_else(|| {
            Err(energyml_geometry::around_well::hole_frame::FrameRefusal::NoDirection { md })
        }),
        Err(why) => Err(why),
    }
    .map_err(|why| why.to_string());
    if let Ok(found) = &hole.frame {
        match path.compass() {
            Some(compass) => match north_in_plane(found, &compass) {
                Ok(north) => hole.north = Some(north),
                Err(why) => hole.north_why = Some(why.to_string()),
            },
            None => hole.north_why = Some("the CRS states no compass for its horizontal axes".to_string()),
        }
    }
    hole
}

/// The values of one property at the MD, and the bounds of its sectors.
#[allow(clippy::too_many_arguments)]
fn read_ring(
    cx: &ViewContext<'_>,
    request: &Request,
    uri: &str,
    frame_uri: &str,
    support: &Support,
    registry: &Interpolations,
    angles_uri: Option<&str>,
    convention: Option<&AngleConvention>,
    north: Option<[f64; 2]>,
    notes: &mut Vec<ViewNote>,
) -> Result<RingData, ViewRefusal> {
    let refuse = |why: String| ViewRefusal::computing(ID, why);
    let settings = &request.settings;
    let object = cx.source.read_object_at(uri).map_err(refuse)?;
    let read = property::read(object.as_reflect()).map_err(|why| refuse(why.to_string()))?;
    let title = read.title.clone().unwrap_or_else(|| read.uuid.clone());

    // The same frame for every ring.
    let frame_uuid = energyml_core::identity::parse_address(frame_uri).0;
    let support_uuid = read.supporting_representation.clone().unwrap_or_default();
    if !support_uuid.eq_ignore_ascii_case(&frame_uuid) {
        return Err(refuse(format!(
            "`{title}` hangs on another frame ({support_uuid}) than the first property ({frame_uuid}): the rings of one target share a frame"
        )));
    }

    let matrix = read_matrix(uri, &read, support.mds.len() as u64, cx.source)
        .map_err(|why| refuse(format!("`{title}`: {why}")))?;
    if matrix.support != RowSupport::Nodes {
        return Err(refuse(format!(
            "`{title}` holds one value per interval of the frame: a target at a MD reads the nodes"
        )));
    }
    if matrix.shape.rank() != 1 || matrix.shape.dims[0] < 3 {
        return Err(refuse(format!(
            "`{title}` has {:?} values per node: a ring takes one dimension of at least three values, one per sector",
            matrix.shape.dims
        )));
    }
    let sectors = matrix.shape.dims[0];

    // What the values are, and the method between two nodes.
    let angle_unit = read
        .uom
        .as_deref()
        .filter(|unit| Dimension::of(unit) == Some(Dimension::PlaneAngle));
    let integer = matches!(
        read.patches
            .first()
            .and_then(|patch| energyml_domain::values::family(patch.array)),
        Some(energyml_domain::Family::Integer)
    );
    let nature = Nature {
        categorical: false,
        boolean: false,
        integer,
        angle: angle_unit.is_some(),
        intervals: false,
    };
    let logarithmic = settings.text("scale") == Some("log");
    let (method_id, _) = default_interpolation(&nature, logarithmic);
    let asked = settings.text("interpolation").unwrap_or(method_id);
    let method = registry.get(asked).ok_or_else(|| ViewRefusal::BadSetting {
        name: "interpolation".to_string(),
        why: format!("no method `{asked}`: the methods are {}", registry.ids().join(", ")),
    })?;

    // The values at the MD: the row of a node, or the two rows between which it falls, interpolated.
    let lower = matrix
        .row(support.placed.lower)
        .ok_or_else(|| refuse(format!("`{title}` has no row {}", support.placed.lower)))?;
    let upper = matrix
        .row(support.placed.upper)
        .ok_or_else(|| refuse(format!("`{title}` has no row {}", support.placed.upper)))?;
    let in_radians = |row: &[f64]| -> Result<Vec<f64>, ViewRefusal> {
        match angle_unit {
            Some(unit) if unit != "rad" => row
                .iter()
                .map(|value| {
                    if value.is_nan() {
                        Ok(*value)
                    } else {
                        units::convert(*value, unit, "rad").map_err(|why| refuse(why.to_string()))
                    }
                })
                .collect(),
            _ => Ok(row.to_vec()),
        }
    };
    let interpolated = interpolate_row(
        &in_radians(lower)?,
        &in_radians(upper)?,
        support.placed.t,
        support.gap,
        &support.rule,
        method,
        &nature,
    )
    .map_err(|why| refuse(format!("`{title}`: {why}")))?;
    let values: Vec<f64> = match angle_unit {
        Some(unit) if unit != "rad" => interpolated
            .values
            .iter()
            .map(|value| {
                if value.is_nan() {
                    Ok(*value)
                } else {
                    units::convert(*value, "rad", unit).map_err(|why| refuse(why.to_string()))
                }
            })
            .collect::<Result<_, _>>()?,
        _ => interpolated.values.clone(),
    };
    if let Some(note) = interpolated.note {
        notes.push(ViewNote {
            key: "interpolation-caveat",
            says: match note {
                InterpNote::FractionalInteger => format!(
                    "`{title}` holds integers and `{asked}` interpolates them: a value between two nodes may be fractional"
                ),
                InterpNote::AngleTakenAsLinear => format!(
                    "`{title}` holds angles and `{asked}` takes them as plain numbers: the wrap at a full turn is ignored"
                ),
            },
        });
    }
    if interpolated.refused > 0 {
        notes.push(ViewNote {
            key: "interpolation-refused",
            says: format!(
                "`{title}`: `{asked}` refused {} of {sectors} sector(s){}",
                interpolated.refused,
                interpolated
                    .first_refusal
                    .as_ref()
                    .map(|why| format!(": {why}"))
                    .unwrap_or_default()
            ),
        });
    }

    // The angles of the sectors, in the file's radians: a paired property, an axis the file states, or sectors
    // the caller declares even.
    let links: Vec<AxisLink> = angles_uri
        .map(|property| {
            vec![AxisLink::PairedProperty {
                property: property.to_string(),
                dim: 0,
            }]
        })
        .unwrap_or_default();
    let resolved = AxisSources::with_builtins()
        .resolve(
            &AxisSubject {
                uri,
                shape: &matrix.shape,
                facets: &read.facets,
                links: &links,
                rows: Some(support.mds.len() as u64),
            },
            cx.source,
            cx.source,
        )
        .map_err(|why| refuse(format!("`{title}`: {why}")))?;
    let axis = &resolved.axes[0];
    let stated: Option<Vec<f64>> = match &axis.values {
        AxisValues::Listed(values) => Some(values.clone()),
        AxisValues::Regular { start, step, count } => Some((0..*count).map(|k| start + k as f64 * step).collect()),
        AxisValues::Labels(_) | AxisValues::Ranks(_) => None,
    };
    let angles: Vec<f64> = match stated {
        Some(values) => {
            let Some(unit) = axis
                .uom
                .as_deref()
                .filter(|unit| Dimension::of(unit) == Some(Dimension::PlaneAngle))
            else {
                return Err(refuse(format!(
                    "the axis of `{title}` is in `{}`, not an angle: the sectors cannot be placed",
                    axis.uom.as_deref().unwrap_or("no unit")
                )));
            };
            values
                .iter()
                .map(|value| units::radians(*value, unit).map_err(|why| refuse(why.to_string())))
                .collect::<Result<_, _>>()?
        }
        None if settings.flag("regular_sectors") => regular_sectors(sectors, 0.0),
        None => {
            return Err(ViewRefusal::MissingSetting {
                view: ID.to_string(),
                name: "angles".to_string(),
                about: format!(
                    "the file states no angles for `{title}`: give the property that holds them as `angles`, or declare the sectors even with `regular_sectors`"
                ),
            });
        }
    };

    // The door of the placement checks the count and the convention once more, on the angles themselves.
    polar_points(&vec![1.0; sectors], &angles, convention, north).map_err(|why| refuse(why.to_string()))?;
    let bounds = sector_bounds(&angles).map_err(|why: PolarRefusal| refuse(why.to_string()))?;

    Ok(RingData {
        uri: uri.to_string(),
        title,
        uom: read.uom.clone(),
        values,
        bounds,
        method: static_id(registry, asked),
        first_refusal: interpolated.first_refusal.map(|why| why.to_string()),
    })
}

/// The id of a method of the registry as a `'static` word.
fn static_id(registry: &Interpolations, id: &str) -> &'static str {
    registry.ids().into_iter().find(|known| *known == id).unwrap_or("custom")
}

#[allow(clippy::too_many_arguments)]
fn picture(
    size: [f64; 2],
    md: f64,
    md_uom: &str,
    support: &Support,
    rings: &[Ring],
    convention: Option<&AngleConvention>,
    hole: &Hole,
    settings: &Settled,
    title: &str,
) -> Result<DisplayList, ViewRefusal> {
    let refuse = |why: String| ViewRefusal::computing(ID, why);
    let convention = convention.ok_or_else(|| refuse("no angle convention is declared".to_string()))?;
    let [width, height] = size;
    let count = rings.len();
    let legend_height = 44.0 + 34.0 * count as f64;
    let radius = ((width - 160.0) / 2.0).min((height - 60.0 - legend_height - 40.0) / 2.0).max(40.0);
    let centre = [width / 2.0, 62.0 + radius];
    let hub = radius * 0.12;
    let band = (radius - hub) / count as f64;
    let gap = if band > 6.0 { 2.0 } else { 0.0 };
    let base = rings[0].data.uri.clone();
    let md_word = short(md);

    let mut list = DisplayList::new(size, PageUnit::Pixel);
    list.title = format!("{ID} - MD {md_word}");
    list.background = Some(style::PAPER);

    let mut header = Layer::new("title");
    header.elements.push(Element::new(vec![style::label_item(
        [24.0, 24.0],
        format!("{title} - MD {md_word} {md_uom}").trim_end().to_string(),
        style::text(style::TITLE, style::INK, Anchor::Start),
    )]));
    list.layers.push(header);

    // The sectors.
    let mut layer = Layer::new("rings");
    for (at, ring) in rings.iter().enumerate() {
        let inner = hub + band * at as f64;
        let outer = inner + band - gap;
        for (k, (value, bounds)) in ring.data.values.iter().zip(&ring.data.bounds).enumerate() {
            let middle = (bounds[0] + bounds[1]) / 2.0;
            let half = (bounds[1] - bounds[0]).abs() / 2.0;
            let direction = plane_direction(middle, convention, hole.north)
                .map_err(|why| refuse(why.to_string()))?;
            // The page has y down: the plane's high side is up the page, and an angle grows clockwise on screen.
            let across = (-direction[1]).atan2(direction[0]);
            let colour = ring.colours.sample(*value);
            let (fill, stroke) = match colour {
                Some(rgba) if value.is_finite() => (
                    Some(Paint::Solid(Colour::rgba(rgba[0], rgba[1], rgba[2], rgba[3]))),
                    Stroke::solid(style::PAPER, 0.5),
                ),
                _ => (None, Stroke::solid(style::NEUTRAL, 0.75)),
            };
            let mut element = Element::keyed(
                format!("{}#md={md_word}#col={k}", ring.data.uri),
                "sector",
                vec![Item::Sector {
                    centre,
                    inner,
                    outer,
                    start: across - half,
                    end: across + half,
                    fill,
                    stroke: Some(stroke),
                }],
            );
            element.title = format!(
                "{}: {}{}",
                ring.data.title,
                if value.is_finite() { format!("{value}") } else { "no value".to_string() },
                ring.data.uom.as_deref().map(|uom| format!(" {uom}")).unwrap_or_default()
            );
            element.data = vec![
                ("value".to_string(), if value.is_finite() { format!("{value}") } else { "none".to_string() }),
                ("ring".to_string(), at.to_string()),
                ("sector".to_string(), k.to_string()),
            ];
            layer.elements.push(element);
        }
    }
    list.layers.push(layer);

    // The circles that bound the rings, and the well at the centre.
    let mut graticule = Layer::new("graticule");
    let mut circles = Vec::new();
    if settings.flag("rings") {
        circles.push(Item::Circle {
            centre,
            radius: hub,
            fill: None,
            stroke: Some(Stroke::solid(style::MUTED, 0.75)),
        });
        for at in 0..count {
            circles.push(Item::Circle {
                centre,
                radius: hub + band * (at as f64 + 1.0) - gap,
                fill: None,
                stroke: Some(Stroke::solid(style::MUTED, 0.75)),
            });
        }
    }
    circles.push(Item::Symbol {
        at: centre,
        kind: SymbolKind::Point,
        size: 5.0,
        fill: Some(style::SUBJECT),
        stroke: style::SUBJECT,
    });
    graticule.elements.push(Element::new(circles));
    list.layers.push(graticule);

    // The marks: the high side and the north, only where the frame of the hole is known.
    let mut marks = Layer::new("marks");
    if hole.frame.is_ok() {
        marks.elements.push(Element::keyed(
            format!("{base}#md={md_word}#high-side"),
            "high-side",
            vec![
                Item::Symbol {
                    at: [centre[0], centre[1] - radius - 8.0],
                    kind: SymbolKind::TriangleUp,
                    size: 8.0,
                    fill: Some(style::INK),
                    stroke: style::INK,
                },
                style::label_item(
                    [centre[0], centre[1] - radius - 18.0],
                    "high side",
                    style::text(style::TEXT, style::INK, Anchor::Middle),
                ),
            ],
        ));
        if settings.flag("north")
            && let Some(north) = hole.north
        {
            let length = north[0].hypot(north[1]);
            if length > 0.0 {
                let toward = [north[0] / length, -north[1] / length];
                let at = |distance: f64| [centre[0] + toward[0] * distance, centre[1] + toward[1] * distance];
                marks.elements.push(Element::keyed(
                    format!("{base}#md={md_word}#north"),
                    "north",
                    vec![
                        Item::Polyline {
                            points: vec![at(radius + 6.0), at(radius + 28.0)],
                            stroke: Stroke::solid(style::SUBJECT, 2.0),
                        },
                        style::label_item(
                            [at(radius + 40.0)[0], at(radius + 40.0)[1] + 4.0],
                            "N",
                            style::text(style::TEXT, style::SUBJECT, Anchor::Middle),
                        ),
                    ],
                ));
            }
        }
    }
    list.layers.push(marks);

    // The legend: where the MD falls, the method, and for each ring its range and its colours.
    let mut legend = Layer::new("legend");
    let mut top = centre[1] + radius + 44.0;
    let nearest = nearest_node(&support.mds, md);
    let position = match support.placed.on_node() {
        Some(node) => format!("MD {md_word} {md_uom}: node {node}, no interpolation"),
        None => format!(
            "MD {md_word} {md_uom}: interpolated, t = {} between {} and {} {md_uom}{}",
            short(support.placed.t),
            short(support.mds[support.placed.lower]),
            short(support.mds[support.placed.upper]),
            nearest.map(|node| format!(" (nearest node {node})")).unwrap_or_default()
        ),
    };
    let convention_words = format!(
        "angles from {}, {}, {} (declared)",
        origin_words(convention.origin),
        match convention.sense {
            AngleSense::Clockwise => "clockwise",
            AngleSense::Anticlockwise => "anticlockwise",
        },
        match convention.viewed_from {
            ViewedFrom::LookingDownhole => "looking downhole",
            ViewedFrom::LookingUphole => "looking uphole",
        }
    );
    legend.elements.push(Element::new(vec![
        style::label_item([24.0, top - 22.0], position.trim_end().to_string(), style::text(style::TEXT, style::INK, Anchor::Start)),
        style::label_item(
            [24.0, top - 8.0],
            format!(
                "{convention_words}; method {}",
                rings.iter().map(|ring| ring.data.method).next().unwrap_or("none")
            ),
            style::text(style::TEXT, style::MUTED, Anchor::Start),
        ),
    ]));
    for (at, ring) in rings.iter().enumerate() {
        let uom = ring.data.uom.as_deref().map(|uom| format!(" {uom}")).unwrap_or_default();
        let mut items = vec![style::label_item(
            [24.0, top + 8.0],
            format!(
                "ring {} (from the hole outwards): {}, drawn {} to {}{uom}; {}",
                at + 1,
                ring.data.title,
                short(ring.range.0),
                short(ring.range.1),
                ring.colours.source.says()
            ),
            style::text(style::TEXT, style::INK, Anchor::Start),
        )];
        let (low, high) = ring.scale;
        for step in 0..BAR_STEPS {
            let fraction = step as f64 / (BAR_STEPS - 1) as f64;
            let value = if settings.text("scale") == Some("log") {
                (low.ln() + fraction * (high.ln() - low.ln())).exp()
            } else {
                low + fraction * (high - low)
            };
            let fill = ring
                .colours
                .sample(value)
                .map(|rgba| Paint::Solid(Colour::rgba(rgba[0], rgba[1], rgba[2], rgba[3])));
            items.push(Item::Rect {
                rect: Rect {
                    x: 24.0 + step as f64 * 8.0,
                    y: top + 14.0,
                    w: 8.0,
                    h: 8.0,
                },
                corner: 0.0,
                fill,
                stroke: None,
            });
        }
        items.push(style::label_item(
            [24.0 + BAR_STEPS as f64 * 8.0 + 6.0, top + 22.0],
            format!("{} to {}{uom}", short(low), short(high)),
            style::text(style::SMALL, style::MUTED, Anchor::Start),
        ));
        legend
            .elements
            .push(Element::keyed(format!("{}#legend", ring.data.uri), "legend", items));
        top += 34.0;
    }
    list.layers.push(legend);
    Ok(list)
}

fn origin_words(origin: AngleOrigin) -> &'static str {
    match origin {
        AngleOrigin::HighSide => "the high side",
        AngleOrigin::LowSide => "the low side",
        AngleOrigin::Right => "the right",
        AngleOrigin::Left => "the left",
        AngleOrigin::North => "grid north",
    }
}

/// A number written short: no trailing zeros, two decimals at most.
fn short(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}
