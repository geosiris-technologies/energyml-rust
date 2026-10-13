//! `isocontours` - the contour lines of a surface, as polylines.
//!
//! A viewer used to compute them with a button of its own,
//! because the module did not know how; this is the second half of the same
//! gap that `triangulate` has just filled. The need: contours should rather
//! only be in the list of possible operations.
//!
//! # What is contoured
//!
//! **A field laid on the shape if there is one, the elevation otherwise** - the same
//! rule as `interpolate`, and for the same reason: *what is being mapped*
//! is data of the bag, and assuming the elevation would contour the depth
//! of a surface whose porosity was wanted, without anything failing.
//!
//! # What it does not do itself
//!
//! The computation is [`energyml_analysis::isocontour`] - the levels, the segments,
//! the chaining - and the writing is that of [`super::polylines`]: a contour
//! line is a set of polylines, some of them closed, and a second
//! writer of the same object would be a second place for the same mistakes.
//!
//! # Curves, bands, or both
//!
//! The need: 2 checkboxes, one to know whether to create the polylines in the
//! model, the other whether to create the property; at least one of the two
//! must be ticked. The bands used to be a front end command on a contour artefact,
//! so **a contour written directly into the package never led to one** -
//! putting the colour on the surface required going through *keep as artefact*.
//! `bands` writes in the same plan:
//!
//! - the **property** - the contoured values, one per node of the surface - by
//!   [`super::property::write`];
//! - the **map** - a sharp band per interval, starting at each level
//!   cut - by [`super::colormap::write`];
//! - and the **entry** that links them, written by `energyml_domain::color` as
//!   everywhere. In the package's colour set if it has one (rewritten under its
//!   uuid, `apply` replaces), in a new set if it has none, and **refused
//!   if it has several**: the standard does not say which one decides a colour, and
//!   choosing one here would decide over someone's data - the rule the front end's
//!   command already held.
//!
//! The colours come from the caller (`colours`, a ramp): this workspace
//! ships no palette. It is sampled **by rank** over each
//! band on its own extent, because the levels are only known here.
//!
//! # References
//!
//! - Marching triangles, as implemented in `energyml-geostat` (`isocontour`).
//!   TODO(reference): verify - marching triangles is named without a citation; add a reference once checked.

use energyml_analysis::isocontour;
use energyml_domain::color::{self, Decorated, Scaling};
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::json;

use crate::declare::{
    Field as FieldIn, Inputs, ObjectIn, Params, PropertyKindRef, Ramp, Shape, Suggestions, Title,
};
use crate::operation::{
    About, Category, Context, Need, NewObject, Note, Operation, Plan, Purpose, Refusal, Slot,
};
use crate::pieces::{Known, Pieces};

use super::polylines::PolylinesInputs;
use super::written::Written;

/// The operation.
pub struct Isocontours;

/// What `isocontours` is told.
///
/// **Not `closed`**: it belongs to the computation - a contour is closed where
/// it closes, and saying so in its place would write rings the surface does
/// not have.
#[derive(Debug, Params)]
pub struct IsocontoursParams {
    /// write the contour lines into the model, as a polyline set
    #[param(default = true, optional)]
    pub lines: bool,
    /// write the contoured values into the model too - a continuous property on the surface, drawn in flat colour bands that start at each level
    #[param(default = false, optional)]
    pub bands: bool,
    /// The frame, what is represented, the naming - for the polyline writer.
    #[param(flatten, when = "lines: true")]
    pub written: Written,
    /// how many **intervals** to cut the range into - one level fewer than that is drawn
    #[param(default = 10.0, optional, min = 1.0, max = 1000.0)]
    pub count: f64,
    /// What the bands are written as.
    #[param(flatten, when = "bands: true")]
    pub banded: BandsParams,
}

/// What `bands` writes - asked only when it is ticked.
#[derive(Debug, Clone, Params)]
pub struct BandsParams {
    /// what the contoured values measure - a property kind of the package or of the standard dictionary; left out, `elevation` or `depth` from the way the surface's frame counts z
    #[param(group = "naming")]
    pub property_kind: Option<PropertyKindRef>,
    /// the unit of the contoured values - an Energistics uom code; left out, the field's, or the vertical unit of the surface's frame
    #[param(group = "naming")]
    pub uom: Option<String>,
    /// the property's title; left out, the surface's and what is contoured
    #[param(group = "naming")]
    pub bands_title: Option<Title>,
    /// the external file the property's values go into
    #[param(default = String::from("data.h5"), group = "naming")]
    pub bands_file: String,
}

/// What `isocontours` takes.
#[derive(Debug, Inputs)]
pub struct IsocontoursInputs {
    /// the surface to contour
    #[input(topology = "triangles")]
    pub shape: Shape,
    /// the values to contour, instead of the elevation
    ///
    /// Optional, and its absence is an answer: the surface's z.
    pub field: Option<FieldIn>,
    /// with `bands`: the colours, a ramp sampled once per band over its own range - the palette of whoever asks, since this workspace ships none
    #[input(palette)]
    pub colours: Option<Ramp>,
    /// with `bands`: the graphical information set the colour link goes into; left empty, the package's own - none, a new set is written; several, refused until one is named
    #[input(types = "eml23.GraphicalInformationSet", every)]
    pub colour_set: Vec<ObjectIn>,
}

/// The surface to contour, and the values read on it.
struct Field {
    name: String,
    points: Vec<f64>,
    triangles: Vec<u32>,
    values: Vec<f64>,
    /// The values come from a field, rather than from the third coordinate.
    from_field: bool,
}

fn field_of(inputs: &IsocontoursInputs) -> Option<Field> {
    let mut points: Vec<f64> = Vec::new();
    let mut triangles: Vec<u32> = Vec::new();
    let mut z: Vec<f64> = Vec::new();
    let mut base = 0u32;
    for patch in &inputs.shape.geometry.patches {
        let Topology::Triangles(faces) = &patch.topology else {
            continue;
        };
        for point in &patch.points {
            points.extend_from_slice(point);
            z.push(point[2]);
        }
        // **Offset by the rank of the patch's first node.** A patch's indices
        // are local to it; contouring two patches laid end to end without this
        // offset would trace the second on the first's nodes.
        for face in faces {
            triangles.extend(face.iter().map(|&i| i + base));
        }
        base += patch.points.len() as u32;
    }
    if triangles.is_empty() {
        return None;
    }
    let field = inputs.field.as_ref().map(|field| field.values.clone());
    let from_field = field.is_some();
    let values = field.unwrap_or(z);
    Some(Field {
        name: inputs.shape.name.clone(),
        points,
        triangles,
        values,
        from_field,
    })
}

impl Operation for Isocontours {
    type Params = IsocontoursParams;
    type Inputs = IsocontoursInputs;

    fn about(&self) -> About {
        About {
            name: "isocontours",
            describe: "the contour lines of a surface, as closed and open polylines",
            category: Category::Geometry,
            makes: &[
                "resqml22.PolylineSetRepresentation",
                "resqml22.ContinuousProperty",
                "resqml22.ContinuousColorMap",
                "eml23.GraphicalInformationSet",
            ],
            recipe: "isocontours",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &IsocontoursInputs) -> Result<(), Refusal> {
        if field_of(inputs).is_none() {
            return Err(Refusal::on(
                "shape",
                "an iso-contour is drawn across faces, and this shape has no triangle - `triangulate` makes them from a cloud",
            ));
        }
        // A field over another piece is not the values of this surface: said,
        // rather than quietly contouring the elevation instead.
        if let Some(field) = &inputs.field
            && field.over != inputs.shape.name
        {
            return Err(Refusal::on(
                "field",
                format!(
                    "lies over `{}`, and the surface is `{}`",
                    field.over, inputs.shape.name
                ),
            ));
        }
        // The contour is drawn across faces, from values read at
        // their nodes - `field_of` reads `field.values` straight as one
        // number per node. A field over cells has one value per triangle, not
        // per node: read the same way, every value would land on the wrong
        // point, silently, because the two arrays can have the same length by
        // coincidence. There is no transfer from cells to nodes in this
        // workspace yet (the crate's rules name none), so
        // this is refused rather than guessed at.
        if let Some(field) = &inputs.field
            && field.on != "nodes"
        {
            return Err(Refusal::on(
                "field",
                format!(
                    "lies on `{}`, and a contour reads values at nodes - no transfer from `{}` to nodes is offered here",
                    field.on, field.on
                ),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: IsocontoursInputs,
        params: IsocontoursParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let found =
            field_of(&inputs).ok_or_else(|| Refusal::on("shape", "no triangulated shape to contour"))?;
        let count = params.count;
        let Some((low, high)) = isocontour::range(&found.values) else {
            return Err(Refusal::on(
                "shape",
                format!(
                    "every node of `{}` is undefined; there is no range to contour",
                    found.name
                ),
            ));
        };
        let levels = isocontour::levels(low, high, count as usize).map_err(|e| Refusal::on("count", e))?;
        if levels.is_empty() {
            // **An answer, not a failure.** A flat surface has a single
            // value: there is no level between `low` and `high` because they
            // are equal. An empty plan would read as a computation that failed.
            return Err(Refusal::on(
                "shape",
                format!(
                    "`{}` is flat at {low}: there is no interval to cut, so no contour to draw",
                    found.name
                ),
            ));
        }

        let mut lines: Vec<Vec<[f64; 3]>> = Vec::new();
        let mut closed: Vec<bool> = Vec::new();
        let mut skipped = 0usize;
        for level in &levels {
            let contour = isocontour::contour(&found.points, &found.triangles, &found.values, *level)
                .map_err(|e| Refusal::new("refused", e.to_string()))?;
            skipped += contour.skipped;
            for line in isocontour::chain(&contour.segments) {
                lines.push(line.points);
                closed.push(line.closed);
            }
        }
        if lines.is_empty() {
            return Err(Refusal::on(
                "shape",
                format!(
                    "no contour crosses `{}` at any of its {} level(s) - every triangle is on one side",
                    found.name,
                    levels.len()
                ),
            ));
        }

        // **Each line's level is not written, and that is a named debt.**
        // RESQML would say it with a discrete property laid on the
        // polylines; without it, a set of curves does not say which is at
        // which elevation. The levels are in the note and the activity, so
        // nothing is lost - but the object, alone, does not carry them.
        let runs: Vec<u32> = lines.iter().map(|line| line.len() as u32).collect();
        let name = format!("isocontours of {}", found.name);
        let shape = Shape {
            name: name.clone(),
            known: None,
            geometry: Geometry {
                kind: "isocontours",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: lines.into_iter().flatten().collect(),
                    topology: Topology::Polylines(runs),
                    closed: Vec::new(),
                }],
            },
            // **Closure comes from the chaining, never from a choice.** A contour
            // line that goes round a dome is closed, one that leaves
            // through the edge is not, and both are in the same set.
            closed: Some(closed.clone()),
        };
        if !params.lines && !params.bands {
            return Err(Refusal::on(
                "lines",
                "nothing to write: tick `lines` (the contour lines, as a polyline set), `bands` (the values on the surface, in colour bands), or both",
            ));
        }
        let title = params.written.naming.title();
        let polylines = PolylinesInputs {
            shape: vec![shape],
            features: None,
        };
        let mut plan =
            super::polylines::write(&polylines, &params.written, None, &format!("{name}:{title}"), cx)?;
        if !params.lines {
            // **The lines are computed, not written**: the shape stays - it is
            // what a scene draws and keeps - and nothing of the polyline set's
            // writing does, a missing frame included, which only the writing of
            // the lines needed.
            plan.objects.clear();
            plan.datasets.clear();
            plan.blocked = None;
            plan.says.retain(|note| !note.key.starts_with("frame-"));
            for made in &mut plan.shapes {
                made.object = None;
            }
        }
        if params.bands {
            bands(&inputs, &params, &found, &levels, &mut plan, cx)?;
        }

        plan.activity.recipe = "isocontours";
        plan.activity.title = format!("isocontours → {title}");
        plan.activity.parameters.push(Parameter::Double {
            title: "count".to_string(),
            value: count,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "levelCount".to_string(),
            value: levels.len() as f64,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "low".to_string(),
            value: levels[0],
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "high".to_string(),
            value: levels[levels.len() - 1],
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Text {
            title: "contoured".to_string(),
            value: if found.from_field {
                "a field of the bag"
            } else {
                "the elevation"
            }
            .to_string(),
        });
        // **The levels actually cut, one parameter each.**
        //
        // They are not `count` regular values: the module aligns them on
        // zero so that a legend carries round numbers, so deriving them again
        // elsewhere would make them drift. They go out **typed**, because a
        // legend and a band map read them as numbers; eml 2.3
        // explicitly allows several values under one name.
        for level in &levels {
            plan.activity.parameters.push(Parameter::Double {
                title: "level".to_string(),
                value: *level,
                uom: None,
            });
        }
        // And the same list in a sentence, for whoever reads the quote. Rendered from
        // the **same** vector, on the line next to it: two renderings of a value
        // computed once cannot diverge.
        plan.says.push(Note {
            key: "levels",
            detail: levels
                .iter()
                .map(|l| format!("{l}"))
                .collect::<Vec<_>>()
                .join(", "),
        });
        if skipped > 0 {
            // A triangle no level crosses cleanly - an undefined
            // node, a degenerate face. Staying silent would make a
            // holed curve look whole.
            plan.says.push(Note {
                key: "triangles-skipped",
                detail: format!("{skipped} triangle(s) were skipped: a node of theirs is undefined"),
            });
        }
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(
                name,
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                about,
            )
        };
        let mut slots =
            crate::operation::AnyOperation::reports(&crate::operation::Typed(super::polylines::Polylines));
        // **Not `count`**: it is already declared as an input, and `template.rs`
        // compares by title - the same name on both sides reads as a
        // disagreement of direction. What an input is worth is already in
        // the activity under the parameter the projection declares.
        slots.push(count("levelCount", "levels actually cut"));
        slots.push(
            Slot::many(
                "level",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "each level actually cut",
                1,
            )
            // **The order is data**: a legend prints them from bottom to
            // top, and a band map makes them consecutive intervals.
            .in_order(),
        );
        slots.push(count("low", "the lowest level"));
        slots.push(count("high", "the highest level"));
        slots.push(Slot::required(
            "contoured",
            Need::Choice(&["the elevation", "a field of the bag"]),
            "where the values were read",
        ));
        slots
    }
}

/// The time units a vertical axis counts in - what makes a contoured elevation
/// a travel time, and its kind `two way time`. The Energistics spellings.
const TIME_UNITS: &[&str] = &["s", "ms", "us", "ns", "min", "h"];

/// The kind the values are measured under when none is named, looked up by
/// title in the dictionary - `elevation` for a frame counting up, `depth` for
/// one counting down, `two way time` for one counting in time. A field's
/// values say nothing of what they measure, and a frame that does not say which
/// way z counts leaves nothing to decide from: both are refused by name.
fn kind_of(found: &Field, known: Option<&Known>, uom: &str) -> Result<(serde_json::Value, String), Refusal> {
    if found.from_field {
        return Err(Refusal::on(
            "propertyKind",
            "the contoured values are a field's, and nothing says what they measure: name a `propertyKind`",
        ));
    }
    let title = if TIME_UNITS.contains(&uom) {
        "two way time"
    } else {
        match known.and_then(|known| known.vertical_up) {
            Some(true) => "elevation",
            Some(false) => "depth",
            None => {
                return Err(Refusal::on(
                    "propertyKind",
                    "the surface's frame does not say which way z counts, so neither `elevation` nor `depth` can be told: name a `propertyKind`",
                ));
            }
        }
    };
    let found = energyml_domain::property_kind::with_default(|dictionary| {
        dictionary
            .by_title(title)
            .first()
            .map(|kind| (kind.uuid.clone(), kind.title.clone()))
    });
    let (uuid, named) = found.ok_or_else(|| {
        Refusal::on(
            "propertyKind",
            format!(
                "the dictionary in use holds no kind called `{title}`: name a `propertyKind` of the package"
            ),
        )
    })?;
    Ok((
        json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": named }),
        title.to_string(),
    ))
}

/// The colour of band `rank` of `count`, read on the ramp **at the same place
/// of its own range**: the first band takes its bottom, the last its top. A
/// ramp is a step function - a colour holds from its level to the next - which
/// is how the caller samples a palette into it, densely.
fn band_colour(ramp: &Ramp, rank: usize, count: usize) -> energyml_domain::color::Hsv {
    let low = ramp.levels[0];
    let high = ramp.levels[ramp.levels.len() - 1];
    let t = if count <= 1 {
        0.0
    } else {
        rank as f64 / (count - 1) as f64
    };
    let at = low + t * (high - low);
    let index = ramp.levels.iter().rposition(|level| *level <= at).unwrap_or(0);
    ramp.colours[index].clone()
}

/// The bands - the property, its colour map and the entry joining them -
/// added to `plan`. See the module's doc.
fn bands(
    inputs: &IsocontoursInputs,
    params: &IsocontoursParams,
    found: &Field,
    levels: &[f64],
    plan: &mut Plan,
    cx: &Context<'_>,
) -> Result<(), Refusal> {
    // A plan asked for its geometry writes nothing: the bands are objects only.
    if matches!(cx.purpose, Purpose::Geometry) {
        return Ok(());
    }
    let known = inputs.shape.known.as_ref();
    let Some(surface) = known.and_then(|known| known.reference.as_ref()) else {
        return Err(Refusal::on(
            "bands",
            format!(
                "the bands are a property laid on the surface, and `{}` is not an object of the package - write it first, or untick `bands`",
                found.name
            ),
        ));
    };
    let colours = inputs.colours.as_ref().ok_or_else(|| {
        Refusal::on(
            "colours",
            "the bands need their colours: a ramp, the caller's palette sampled - none is shipped here",
        )
    })?;
    if colours.levels.is_empty() || colours.colours.len() != colours.levels.len() {
        return Err(Refusal::on(
            "colours",
            format!(
                "{} level(s) and {} colour(s): a ramp has one colour per level",
                colours.levels.len(),
                colours.colours.len()
            ),
        ));
    }

    let banded = &params.banded;
    let uom = banded
        .uom
        .clone()
        .or_else(|| inputs.field.as_ref().and_then(|field| field.uom.clone()))
        .or_else(|| {
            if found.from_field {
                None
            } else {
                known.and_then(|known| known.vertical_uom.clone())
            }
        })
        .ok_or_else(|| {
            Refusal::on(
                "uom",
                "nothing says the unit of the contoured values: name a `uom`",
            )
        })?;
    let (kind, what) = match &banded.property_kind {
        Some(given) => {
            let title = given.0["Title"].as_str().unwrap_or("values").to_string();
            (given.0.clone(), title)
        }
        None => kind_of(found, known, &uom)?,
    };
    let title = match &banded.bands_title {
        Some(given) => given.0.clone(),
        None => format!("{} - contoured {what}", surface.title),
    };
    let seed = cx.seed(&title);

    // The property: the contoured values, one per node of the surface - the
    // numbers the levels were cut from, so the bands fall where the lines do.
    let field = FieldIn {
        name: format!("{} bands", found.name),
        over: found.name.clone(),
        on: "nodes".to_string(),
        values: found.values.clone(),
        uom: Some(uom.clone()),
    };
    let written = super::property::write(
        &field,
        known,
        &super::property::PropertyParams {
            representation: crate::declare::ObjectRef(surface.dor()),
            property_kind: PropertyKindRef(kind),
            uom,
            title: Title(title.clone()),
            data_file: banded.bands_file.clone(),
        },
        &format!("isocontours-bands:{seed}"),
        cx,
    )?;
    let property = written
        .objects
        .first()
        .cloned()
        .ok_or_else(|| Refusal::new("refused", "the property writer wrote nothing"))?;

    // The colour map: **the whole range of the values**, cut at the levels. A
    // band starts at the lowest value when it is below the first level - the
    // values down there are painted too, and deserve their own colour - and
    // the last is closed at the highest, so that no band has no width (the last
    // colour of the band map used to be missing).
    let (low, high) = isocontour::range(&found.values).unwrap_or((levels[0], levels[levels.len() - 1]));
    let mut starts: Vec<f64> = Vec::with_capacity(levels.len() + 1);
    if low < levels[0] {
        starts.push(low);
    }
    // A level **at** the highest value starts a band that holds nothing: cut
    // on a range's own ends (a pyramid's top), it made the last colour a band
    // of no width - the one the legend never showed.
    starts.extend(levels.iter().copied().filter(|level| *level < high));
    let band_colours: Vec<_> = (0..starts.len())
        .map(|rank| band_colour(colours, rank, starts.len()))
        .collect();
    let map = super::colormap::write(
        &starts,
        &band_colours,
        Some(high),
        &format!("{title} - bands"),
        &seed,
        cx,
    )?;

    // The entry joining them, in the package's set - or a new one.
    let targets = [Decorated {
        uuid: property.uuid.clone(),
        qualified_type: property.qualified_type.clone(),
        title: property.title.clone(),
    }];
    let drawn_through = Decorated {
        uuid: map.uuid.clone(),
        qualified_type: map.qualified_type.clone(),
        title: map.title.clone(),
    };
    let sets: Vec<&ObjectIn> = inputs
        .colour_set
        .iter()
        // The one object slot also receives what no slot names - a CRS picked
        // in the form: passed over, not refused.
        .filter(|one| {
            one.known
                .reference
                .as_ref()
                .is_some_and(|r| r.qualified_type.ends_with("GraphicalInformationSet"))
        })
        .collect();
    let set = match sets.as_slice() {
        [] => new_set(&targets, &drawn_through, &seed, cx)?,
        [one] => into_set(one, &targets, &drawn_through)?,
        many => {
            return Err(Refusal::on(
                "colourSet",
                format!(
                    "this package holds {} GraphicalInformationSet ({}), and nothing says which one decides a colour - the standard does not: name the one to write in",
                    many.len(),
                    many.iter()
                        .filter_map(|one| one.known.reference.as_ref().map(|r| format!("`{}`", r.title)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    };

    plan.says.push(Note {
        key: "bands",
        detail: format!(
            "{} value(s) on `{}` as `{title}`, drawn through {} band(s) - the entry {} `{}`",
            found.values.len(),
            surface.title,
            starts.len(),
            if sets.is_empty() {
                "in a new set"
            } else {
                "added to"
            },
            set.title
        ),
    });
    plan.objects.extend(written.objects);
    plan.datasets.extend(written.datasets);
    plan.says.extend(written.says);
    plan.objects.push(map);
    plan.objects.push(set);
    Ok(())
}

/// A `GraphicalInformationSet` holding the one entry - the package had none.
fn new_set(
    targets: &[Decorated],
    map: &Decorated,
    seed: &str,
    cx: &Context<'_>,
) -> Result<NewObject, Refusal> {
    let mut rank = 0usize;
    let uuid = super::support::next_uuid(cx.identity, &format!("isocontours-colours:{seed}"), &mut rank)?;
    let title = "Graphical Information Set".to_string();
    let set = eml::GraphicalInformationSet {
        uuid: uuid.clone(),
        schema_version: "2.3".to_string(),
        citation: eml::Citation {
            title: title.clone(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        },
        graphical_information: vec![color::colour_map_entry(targets, map, &Scaling::default(), None)],
        ..Default::default()
    };
    Ok(NewObject {
        uuid,
        qualified_type: "eml23.GraphicalInformationSet".to_string(),
        role: "colours".to_string(),
        title,
        json: energyml_core::json::write_value(&set),
    })
}

/// The package's set, with the entry added - rewritten under its own uuid,
/// which `apply` replaces. Added by `energyml_domain::color::set_colour_map`,
/// the writer the front end's colour gesture calls: what it refuses (an object
/// already drawn through another map in this set) is refused here too.
fn into_set(set: &ObjectIn, targets: &[Decorated], map: &Decorated) -> Result<NewObject, Refusal> {
    let reference = set
        .known
        .reference
        .as_ref()
        .ok_or_else(|| Refusal::on("colourSet", "the set has no reference"))?;
    let json = set.known.json.as_ref().ok_or_else(|| {
        Refusal::on(
            "colourSet",
            format!(
                "`{}` came without its content, so no entry can be added to it",
                reference.title
            ),
        )
    })?;
    let mut types = energyml_core::registry::Registry::new();
    energyml_eml_2_3::register(&mut types);
    let mut read = energyml_core::json::read_value(json, &types)
        .map_err(|why| Refusal::on("colourSet", format!("`{}` does not read: {why}", reference.title)))?;
    color::set_colour_map(
        read.root.as_reflect_mut(),
        targets,
        map,
        &Scaling::default(),
        None,
    )
    .map_err(|why| Refusal::on("colourSet", format!("`{}`: {why}", reference.title)))?;
    Ok(NewObject {
        uuid: reference.uuid.clone(),
        qualified_type: reference.qualified_type.clone(),
        role: "colours".to_string(),
        title: reference.title.clone(),
        json: energyml_core::json::write_value(read.root.as_reflect()),
    })
}
