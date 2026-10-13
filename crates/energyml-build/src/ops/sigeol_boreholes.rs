//! `sigeol-boreholes` — the BRGM SIGEOL `Borehole` export, as wellbores.
//!
//! The sibling of [`super::sigeol`], which reads the same producer's
//! `GeologicUnit` export: one operation per vocabulary, both reading
//! [`crate::geojson`]'s features and neither knowing about the other.
//! [`crate::borehole`] is the vocabulary; this is the couture.
//!
//! # What it writes, per hole
//!
//! A `WellboreFeature`, its `WellboreInterpretation`, a
//! `WellboreTrajectoryRepresentation` and a `WellboreFrameRepresentation`, plus
//! the reference point the depths are measured from — a
//! `ReferencePointInALocalEngineeringCompoundCrs`, in the compound — and the
//! frame's CRS objects once for the whole import: the projected and vertical
//! systems and the identity compound joining them, which every geometry names
//! (RESQML TRG 2.2 §3.1, p. 22). The datum comes from
//! [`super::well::datum_in`] and the two representations from
//! [`super::well::write`], the very makers `well-from-polyline` goes through:
//! a second copy of the measured depths, the vertical detection and the
//! parametric line's kind would be a second place for the same mistakes.
//!
//! # The trajectory is built from the depths, not the depths from the trajectory
//!
//! This is the one reversal with respect to `well-from-polyline`, and it is
//! what makes the operation worth writing. A SIGEOL borehole has **no line**:
//! it has a head — a 2D `Point` — an elevation beside it, and a list of
//! intervals. So:
//!
//! 1. the measured depths are the intervals' distinct bounds, sorted, with a
//!    zero prepended when the shallowest is deeper than the head
//!    ([`crate::borehole::Borehole::mds`] — MDs rarely have a value for 0; zero is generally assumed to be the DEM surface);
//! 2. the head is placed into the frame **once**, carrying the elevation as its
//!    third ordinate, so the reprojection sees the one position the file
//!    states rather than a column of copies of it;
//! 3. the control points are that placed head at each depth below the ground.
//!
//! And the depths are then **handed to the writer**, not left for it to
//! rebuild. `well::write` derives them from its line when nobody states them,
//! which is right for a line somebody drew; here it would be a round trip — a
//! subtraction, then an accumulation of chord lengths — and **that round trip
//! is not exact**. A hole whose file says `6.4` came back `6.399999999999999`,
//! which is this crate inventing a value by a detour and putting it where a
//! producer's own is expected. Found by the test that pins the node MDs
//! against the file, which is exactly the assertion the root rule asks for:
//! *the form comes from the spec, the values come from the file*.
//!
//! What the writer **does** decide is the shape: it measures the verticality
//! off the control points it is handed and writes RESQML's own vertical line —
//! kind 0, parameters on the vertical CRS direction. Nothing here declares a
//! hole vertical; one detector serves both operations, and what it says comes
//! back in the plan's notes.
//!
//! # The depths must be in the frame's unit, and the frame's unit is the metre
//!
//! [`crate::crs::build`] writes a `VerticalCrs` in metres, pointing up. A hole
//! whose intervals are in feet would have its geometry built by subtracting
//! feet from metres — a hole three times too short, in a package that opens
//! cleanly. So a unit other than the metre sets the hole aside **by name**
//! rather than being converted here: a conversion is a decision, and the one
//! place it belongs is a reader of units, not an importer.
//!
//! # Where the producer's own facts go
//!
//! The placement:
//!
//! | the file says | it is written as |
//! |---|---|
//! | `dateOfDrilling.endPosition` | `Citation/Creation` — *when the hole was drilled* |
//! | the run's own instant | `Citation/LastUpdate` — when this package was made |
//! | `purpose` (15 of 1 000 holes) | `Citation/Description`, else the file's `description` |
//! | `parameter_id_forage` | the `WellboreFeature`'s **uuid**, verbatim |
//! | `parameter_id_bss` | `Citation/Title`, and an alias |
//! | `parameter_indice_bss`, `identifier` | `Aliases` |
//! | `boreholeLength`, the drilled interval | `ExtensionNameValue`, with their units |
//!
//! And the geology is **not** written. The data file holds nothing to create
//! properties from, since the intervals have no clear description of the
//! type of material they contain, nor any measured borehole
//! property. Each `logElement` carries an identifier and
//! nothing else — no lithology, no measurement — so writing a property from it
//! would put a column of numbers in the package standing for nothing. What is
//! prepared for the day the export carries more is the **reading**:
//! [`crate::borehole::Interval`] keeps every interval, its bounds, its
//! `GeologicUnit` href and the producer's own key, and the frame's nodes are
//! already the interval boundaries — so an interval property lands on the
//! elements that exist, with no second pass over the depths.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use energyml_domain::provenance::Parameter;

use crate::borehole::{self, Borehole};
use crate::declare::{Epsg, Features, Inputs, Params, Suggestions};
use crate::geojson::{Position, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dropped, Identity, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Pieces;

use super::support;
use super::well::{Datum, DatumKind, TrajectoryShape, Wellbore};
use super::written::FrameParams;

/// The operation.
pub struct SigeolBoreholes;

/// What `sigeol-boreholes` is told.
///
/// The same four as [`super::sigeol::SigeolParams`] minus the stage, which is
/// a fact about geologic units and means nothing to a hole. A GeoJSON is WGS 84
/// by definition (RFC 7946), so the source code has a default and the target
/// does not.
#[derive(Debug, Params)]
pub struct BoreholeParams {
    /// the EPSG code the file's coordinates are in — 4326 by definition
    #[param(default = Epsg(4326))]
    pub source_epsg: Epsg,
    /// reproject into this EPSG system, in metres; left out, the degrees are written as they stand
    pub project: Option<Epsg>,
    /// the EPSG code of the vertical system, when known; left out, the package says unknown
    pub vertical: Option<Epsg>,
    /// the external file the coordinates go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What `sigeol-boreholes` takes: the SIGEOL export — **several** collections
/// merged, as its sibling does.
#[derive(Debug, Inputs)]
pub struct BoreholeInputs {
    /// the SIGEOL borehole export, as a feature collection
    #[input(least = 1)]
    pub features: Vec<Features>,
}

/// What the import counted, so that the activity can say it.
#[derive(Default)]
struct Tally {
    boreholes: usize,
    intervals: usize,
    nodes: usize,
    zero_added: usize,
    undated: usize,
    with_gaps: usize,
}

/// The holes set aside, grouped by why — one line each, never a bare count,
/// and **naming the holes**: a borehole refused for having no elevation is a
/// case the synthetic model shows on purpose,
/// and a count of one does not say which of twenty-three it was.
#[derive(Default)]
struct SetAside(BTreeMap<String, (usize, Vec<String>)>);

/// How many names a line of set-aside holes carries before counting the rest.
const NAMED: usize = 10;

impl SetAside {
    fn add(&mut self, why: impl Into<String>, hole: Option<&str>) {
        let entry = self.0.entry(why.into()).or_default();
        entry.0 += 1;
        if let Some(hole) = hole {
            entry.1.push(hole.to_string());
        }
    }

    fn into_dropped(self) -> Vec<Dropped> {
        self.0
            .into_iter()
            .map(|(why, (count, holes))| {
                let why = match holes.len() {
                    0 => why,
                    n if n <= NAMED => format!("{why} — {}", holes.join(", ")),
                    n => format!("{why} — {} and {} more", holes[..NAMED].join(", "), n - NAMED),
                };
                Dropped { count, why }
            })
            .collect()
    }
}

impl Operation for SigeolBoreholes {
    type Params = BoreholeParams;
    type Inputs = BoreholeInputs;

    fn about(&self) -> About {
        About {
            name: "sigeol-boreholes",
            describe: "the boreholes of a BRGM SIGEOL export: a feature, an interpretation, a trajectory and a frame per hole, the measured depths rebuilt from its intervals",
            category: Category::Import,
            // The datum sits in the compound the import creates (decision (b)
            // of 24/09, `well::datum_in`), so it is the compound's form.
            makes: &[
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "resqml22.WellboreFeature",
                "resqml22.WellboreInterpretation",
                "resqml22.WellboreTrajectoryRepresentation",
                "resqml22.WellboreFrameRepresentation",
            ],
            recipe: "sigeol-boreholes",
            // It writes its own pair, carrying the producer's identifiers —
            // so there is nothing for the shared feature/interpretation slots
            // to offer, and nothing is imposed on a slot that does not exist.
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Wells]
    }

    /// Propose a metric system, taken from the boreholes' own coordinates.
    ///
    /// **The asymmetry with `sigeol-units`, found on 19/09**, and it is the
    /// reason a package of needles ever got written: that operation fills a
    /// slot from what it reads, this one filled none. So `project` stayed empty
    /// in the form, the degrees were kept, and every hole came out between a
    /// hundred and ten thousand times longer than the model is wide.
    ///
    /// It is a GeoJSON, so the EPSG codes should be known.
    /// The **source** is, by the format — RFC 7946 fixes WGS 84. The
    /// **target** is a decision, and `utm_epsg_of` is the one proposal that
    /// follows from the numbers rather than from a country: a zone is a
    /// function of the longitude.
    ///
    /// A suggestion and not a default, for the reason the whole crate is built
    /// on: a reprojection moves every coordinate, so it stays visible in the
    /// form and replaceable by a national system. Left empty on purpose, the
    /// plan still refuses and says what to set.
    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let head = bag
            .features()
            .flat_map(|(_, _, features)| features.iter())
            .find(|feature| borehole::is_borehole(&feature.properties))
            .and_then(|feature| feature.positions().next());
        if let Some(found) = head.and_then(|at| crate::crs::utm_epsg_of(at.lon, at.lat)) {
            slots.set("project", serde_json::json!(found));
        }
    }

    /// **The content, not the kind**, exactly as `sigeol-units` argues it: a
    /// feature collection binds, and only a `Borehole` inside it makes the file
    /// an export this reads.
    fn check(&self, inputs: &BoreholeInputs) -> Result<(), Refusal> {
        let mut features = 0usize;
        let mut boreholes = 0usize;
        for collection in &inputs.features {
            for feature in &collection.features {
                features += 1;
                if borehole::is_borehole(&feature.properties) {
                    boreholes += 1;
                }
            }
        }
        if boreholes == 0 {
            return Err(Refusal::on(
                "features",
                format!("none of the {features} feature(s) is a GeoSciML `Borehole` this reader knows"),
            ));
        }
        Ok(())
    }

    fn plan(&self, inputs: BoreholeInputs, params: BoreholeParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        if let Identity::Given(_) = cx.identity {
            return Err(Refusal::new(
                "refused",
                "`sigeol-boreholes` names its objects from the producer's own identifiers, so a second run \
                 of the same file writes the same package; it does not take given uuids",
            ));
        }
        let identity = cx.identity;
        let when = cx.when;
        let mut rank = 0usize;

        // The frame, created here: a GeoJSON is in its own system, never the
        // package's — the argument `support::frame_in` makes for every import.
        let frame = support::frame_in(
            &[],
            &FrameParams {
                crs: None,
                source_epsg: Some(params.source_epsg),
                project: params.project,
                vertical: params.vertical,
            },
            identity,
            when,
            &mut rank,
        )?;
        let (target, from) = frame.place.clone().ok_or_else(|| {
            Refusal::new("sourceEpsg", "the frame was not created here: `sigeol-boreholes` places a GeoJSON's own coordinates")
        })?;
        // **A borehole cannot be written into a geographic system, and the
        // reason is that a borehole *is* a vertical extent.**
        //
        // `crate::crs` already names it — *"the horizontal unit is a degree
        // while the vertical is a metre, which no reader can reconcile"* — and
        // for most imports that is a note one can live with, because a surface
        // written in degrees is still a surface. Here every object is a line
        // whose only length is its depth: the holes of one BRGM export run 25 m
        // to 2 189 m deep across a field **0.25° wide**, so each one comes out
        // between a hundred and ten thousand times longer than the whole model
        // is across. Nothing draws that, and nothing says so — `check` passes,
        // the measured depths read back exactly, the map places every wellhead
        // where the producer put it.
        //
        // Found on 19/09 from a package built in the front end, where `project` was
        // simply left empty. On the 3D view the wells looked gigantic, and the same
        // operation run with `project` showed them fine.
        //
        // So it refuses, and names the one thing to do. Refusing rather than
        // projecting on a guess is the repository's rule: an EPSG code chosen
        // here would move every coordinate by a decision nobody made.
        if let crate::crs::Target::Geographic(code) = &target {
            return Err(Refusal::new(
                "project",
                format!(
                    "a borehole is a vertical extent, and EPSG:{code} measures its horizontal axes in degrees while its depths are linear: every hole would be written between a hundred and ten thousand times longer than the model is wide, which no reader can reconcile — name a projected system in `project` (EPSG:2154 for mainland France)"
                ),
            ));
        }
        // Created here, so metres pointing up — which is what lets a depth be
        // subtracted from an elevation below.
        let vertical_up = Some(true);

        let file = params.data_file.clone();
        let mut objects = frame.objects;
        let mut datasets = Vec::new();
        let mut parameters = frame.parameters;
        let mut says: Vec<Note> = Vec::new();
        let mut aside = SetAside::default();
        // What `well::write` said, by key: how many holes, and one of them
        // verbatim.
        let mut from_writer: BTreeMap<&'static str, (usize, String)> = BTreeMap::new();
        let mut tally = Tally::default();
        let mut source = String::new();

        for collection in &inputs.features {
            if source.is_empty() {
                source = collection.file.clone();
            }
            for feature in &collection.features {
                if !borehole::is_borehole(&feature.properties) {
                    aside.add("not a `Borehole`: the feature is of another kind", feature.id.as_deref());
                    continue;
                }
                let head = match feature.shape {
                    Shape::Points => feature.parts.first().and_then(|part| part.first()).copied(),
                    _ => None,
                };
                let Some(head) = head else {
                    aside.add("a borehole with no head: its geometry is not a `Point`", feature.id.as_deref());
                    continue;
                };
                let Some(hole) = borehole::borehole_of(feature.id.as_deref(), Some((head.lon, head.lat)), &feature.properties)
                else {
                    aside.add("a `Borehole` whose properties this reader does not recognise", feature.id.as_deref());
                    continue;
                };
                let (Some(key), Some(title)) = (hole.key(), hole.title()) else {
                    aside.add("a borehole with no identifier at all: nothing can name its objects", None);
                    continue;
                };
                let Some(elevation) = hole.elevation else {
                    aside.add("no `referenceLocation.elevation`: the ground is unknown, and the depths start there", Some(title.as_str()));
                    continue;
                };
                let mds = match hole.mds() {
                    Ok(mds) => mds,
                    Err(why) => {
                        aside.add(why, Some(title.as_str()));
                        continue;
                    }
                };
                // The frame's vertical axis is metres (`crs::build`). Converting
                // here would be a decision taken in an importer; naming the
                // unit sends it to whoever owns that decision.
                if mds.uom != "m" {
                    aside.add(format!(
                        "the depths are in `{}` and the frame's vertical axis is the metre: convert the export, or the hole would be written at the wrong depth",
                        mds.uom
                    ), Some(title.as_str()));
                    continue;
                }
                let placed = match crate::crs::place(&[Position { lon: head.lon, lat: head.lat, z: Some(elevation) }], &target, from)
                {
                    Ok(placed) => placed,
                    Err(why) => {
                        aside.add(why, Some(title.as_str()));
                        continue;
                    }
                };
                let [x, y, ground] = placed[0];

                // The control points: the placed head, at each depth below the
                // ground. `well::write` measures the verticality back off
                // these and writes RESQML's vertical line itself.
                let line: Vec<[f64; 3]> = mds.values.iter().map(|md| [x, y, ground - md]).collect();

                let drilled = hole.drilled_at();
                if drilled.is_none() {
                    tally.undated += 1;
                }
                if mds.zero_added {
                    tally.zero_added += 1;
                }
                if !mds.gaps.is_empty() {
                    tally.with_gaps += 1;
                }
                let creation = drilled.unwrap_or_else(|| when.creation.clone());
                let description = describe(&hole);
                let cite = |name: &str| -> Value {
                    let mut citation = json!({
                        "$type": "eml23.Citation",
                        "Title": name,
                        "Originator": when.originator,
                        "Format": energyml_core::CITATION_FORMAT,
                        // What the producer says: when the hole was drilled.
                        "Creation": creation,
                        // What this run says: when the package was written.
                        "LastUpdate": when.creation,
                    });
                    if let Some(description) = &description {
                        citation["Description"] = json!(description);
                    }
                    citation
                };

                // -- the feature ---------------------------------------------
                //
                // Its uuid is `parameter_id_forage` **verbatim** when that is a
                // uuid: it is a permanent identifier the producer owns, and a
                // consumer holding the BRGM's own key finds the object by it.
                // The same argument `naming.rs` makes for a unit's referential
                // uuid, one step stronger — here the identifier already *is* a
                // uuid, so nothing has to be derived from it.
                let feature_uuid = match hole.producer_uuid() {
                    Some(uuid) => uuid.to_string(),
                    None => support::next_uuid(identity, &format!("well-feature:{key}"), &mut rank)?,
                };
                objects.push(NewObject {
                    uuid: feature_uuid.clone(),
                    qualified_type: "resqml22.WellboreFeature".to_string(),
                    role: "feature".to_string(),
                    title: title.clone(),
                    json: json!({
                        "$type": "resqml22.WellboreFeature",
                        "uuid": feature_uuid,
                        "schemaVersion": "2.2",
                        "Citation": cite(&title),
                        // The BSS is a published national register, so a hole in
                        // it *is* well known — the one place this can honestly
                        // say true where `support::represented_in` says false.
                        "IsWellKnown": true,
                        "Aliases": aliases(&hole),
                        "ExtensionNameValue": extensions(&hole),
                    }),
                });

                // -- its interpretation ---------------------------------------
                let interpretation_uuid = support::next_uuid(identity, &format!("well-interpretation:{key}"), &mut rank)?;
                let interpretation_title = format!("{title} — interpretation");
                objects.push(NewObject {
                    uuid: interpretation_uuid.clone(),
                    qualified_type: "resqml22.WellboreInterpretation".to_string(),
                    role: "interpretation".to_string(),
                    title: interpretation_title.clone(),
                    json: json!({
                        "$type": "resqml22.WellboreInterpretation",
                        "uuid": interpretation_uuid,
                        "schemaVersion": "2.2",
                        "Citation": cite(&interpretation_title),
                        "Domain": "depth",
                        // The export states a drilled interval and a date of
                        // drilling: this hole exists in the ground.
                        "IsDrilled": true,
                        "InterpretedFeature": {
                            "$type": "eml23.DataObjectReference",
                            "Uuid": feature_uuid,
                            "QualifiedType": "resqml22.WellboreFeature",
                            "Title": title,
                        },
                    }),
                });
                let interpretation = json!({
                    "$type": "eml23.DataObjectReference",
                    "Uuid": interpretation_uuid,
                    "QualifiedType": "resqml22.WellboreInterpretation",
                    "Title": interpretation_title,
                });

                // -- the datum, the trajectory and the frame -------------------
                let (datum, datum_objects) = super::well::datum_in(
                    &Datum {
                        named: None,
                        head: [x, y],
                        ground,
                        // The elevation is the ground at the head: the MNT, as
                        // the rule reads it.
                        kind: DatumKind::GroundLevel,
                        crs: &frame.crs,
                        vertical_crs: frame.vertical.as_ref(),
                        title: &title,
                        seed: &key,
                        citation: &cite,
                    },
                    identity,
                    &mut rank,
                )?;
                objects.extend(datum_objects);
                let written = super::well::write(
                    &Wellbore {
                        title: &title,
                        seed: &key,
                        line: &line,
                        // The file's own depths, not depths rebuilt from the
                        // line that was built from them — see `Wellbore::mds`.
                        mds: Some(&mds.values),
                        ground,
                        uom: &mds.uom,
                        shape: TrajectoryShape::Linear,
                        tangents: None,
                        // One frame node per control point — and the control
                        // points *are* the depths the file states. A spacing
                        // would put nodes where nothing was measured.
                        spacing: None,
                        datum: &datum,
                        interpretation: &interpretation,
                        file: &file,
                        vertical_up,
                        crs: &frame.crs,
                        citation: &cite,
                        // Nothing to resume: this operation creates, it does not take up again.
                        keep: None,
                    },
                    identity,
                    &mut rank,
                )?;
                tally.boreholes += 1;
                tally.intervals += hole.intervals.len();
                tally.nodes += written.nodes;
                // **What the writer said is kept, grouped by its key.** One
                // note per hole would be a thousand notes saying the same
                // thing, and dropping them would lose the one fact this
                // operation cannot state itself: the writer, not this module,
                // is what decides a hole is vertical.
                for note in written.says {
                    let entry = from_writer.entry(note.key).or_insert((0usize, note.detail));
                    entry.0 += 1;
                }
                objects.extend(written.objects);
                datasets.extend(written.datasets);
            }
        }

        if tally.boreholes == 0 {
            // **The refusal carries the reasons, because there is no plan to
            // carry them.** `dropped` is where a hole set aside is said, and a
            // plan is exactly what is not returned here — so saying *"the plan
            // says why"* pointed at nothing, and a caller learnt only that
            // something went wrong. Found on 19/09 against a plain WFS answer,
            // whose boreholes carry no interval at all.
            // The same lines as `dropped`, holes named — one implementation.
            let why = aside
                .into_dropped()
                .iter()
                .map(|dropped| format!("{} × {}", dropped.count, dropped.why))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Refusal::on(
                "features",
                format!("no borehole could be written — every one was set aside: {why}"),
            ));
        }
        // Counted rather than said one by one: a note per hole would be a
        // thousand notes saying the same thing, and a count is what a reader
        // can act on.
        if tally.zero_added > 0 {
            says.push(Note {
                key: "md-zero-added",
                detail: format!(
                    "{} borehole(s) state no depth at the ground: a zero was prepended to their measured depths, the head standing for the MNT",
                    tally.zero_added
                ),
            });
        }
        if tally.undated > 0 {
            says.push(Note {
                key: "no-drilling-date",
                detail: format!(
                    "{} borehole(s) carry no `dateOfDrilling`: their `Creation` is this run's instant and not the day they were drilled",
                    tally.undated
                ),
            });
        }
        if tally.with_gaps > 0 {
            says.push(Note {
                key: "interval-gaps",
                detail: format!(
                    "{} borehole(s) have a depth range no interval covers: the frame's nodes skip it, as the file does",
                    tally.with_gaps
                ),
            });
        }
        for (key, (count, detail)) in from_writer {
            says.push(Note {
                key,
                detail: format!("{count} borehole(s): {detail}"),
            });
        }
        says.push(Note {
            key: "dates-widened",
            detail: "`dateOfDrilling` is a date and `Creation` a date-time: each was widened to midnight UTC on that day".to_string(),
        });

        let text = |title: &str, value: String| Parameter::Text { title: title.to_string(), value };
        let number = |title: &str, value: f64| Parameter::Double { title: title.to_string(), value, uom: None };
        parameters.extend([
            text("source", source.clone()),
            text("dataFile", file),
            number("boreholeCount", tally.boreholes as f64),
            number("intervalCount", tally.intervals as f64),
            number("frameNodeCount", tally.nodes as f64),
            number("mdZeroAdded", tally.zero_added as f64),
        ]);
        Ok(Plan {
            media: Vec::new(),
            blocked: frame.blocked,
            objects,
            datasets,
            // **No shape**, and the debt is that of `sigeol-units`: this
            // import returns thousands of objects, and the tree of artefacts that
            // matches them will be described by a later design. What is written
            // here is moreover a *parametric line*, whose control
            // points are the trajectory only when it is linear —
            // the reason `well-from-polyline` already gives.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "sigeol-boreholes",
                title: format!("sigeol-boreholes → {source}"),
                seed: source,
                parameters,
            },
            says,
            dropped: aside.into_dropped(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(name, Need::Number { uom: None, least: None, most: None }, about)
        };
        support::frame_reports(vec![
            Slot::required("source", Need::Text, "the file the boreholes came from"),
            count("boreholeCount", "boreholes written"),
            count("intervalCount", "intervals their depths were read from"),
            count("frameNodeCount", "frame nodes written, over every hole"),
            count("mdZeroAdded", "boreholes whose measured depths were given a zero at the ground"),
        ])
    }
}

/// `Citation/Description`: what the hole was for, else what the file calls it.
///
/// The need: put the purpose in the Citation.Description instead.
/// The export's own `description` is `EPOS Borehole` on every one of the 1 000
/// holes — the name of the collection, not of the hole — so it is the fallback
/// and never joined to a purpose: two sources in one sentence would read as one
/// statement the producer never made.
fn describe(hole: &Borehole) -> Option<String> {
    if !hole.purposes.is_empty() {
        return Some(hole.purposes.join(", "));
    }
    hole.description.clone().filter(|text| !text.trim().is_empty())
}

/// The identifiers the producer already owns, as `ObjectAlias`.
///
/// Each carries its **authority**, which is what makes an alias resolvable:
/// `BSS000LKCG` means one thing in the BRGM's register and nothing anywhere
/// else. The kinds are EML's own closed list — a regulatory identifier for the
/// national code, an industry code for the map-sheet index, a unique identifier
/// for the URL that dereferences.
fn aliases(hole: &Borehole) -> Vec<Value> {
    let mut out = Vec::new();
    let mut alias = |identifier: &str, kind: &str, authority: &str| {
        // `ObjectAlias/Identifier` is capped at 64 characters by the schema. A
        // longer one is left out rather than cut: half a URL resolves to
        // nothing and looks exactly like a whole one.
        if !identifier.is_empty() && identifier.chars().count() <= 64 {
            out.push(json!({
                "$type": "eml23.ObjectAlias",
                "Identifier": identifier,
                "IdentifierKind": kind,
                "authority": authority,
            }));
        }
    };
    if let Some(bss) = &hole.bss {
        alias(bss, "regulatory identifier", "BRGM/BSS");
    }
    if let Some(indice) = &hole.indice {
        alias(indice, "industry code", "BRGM/BSS");
    }
    if let Some(identifier) = &hole.identifier {
        alias(identifier, "unique identifier", "data.geoscience.fr");
    }
    out
}

/// What the export measures about the hole itself, with its units.
///
/// `ExtensionNameValue` and not a property: these are facts about the
/// **object**, one value each, and a property is a column over elements.
fn extensions(hole: &Borehole) -> Vec<Value> {
    let mut out = Vec::new();
    let mut measure = |name: &str, value: f64, uom: &str| {
        out.push(json!({
            "$type": "eml23.ExtensionNameValue",
            "Name": name,
            "Value": { "$type": "eml23.StringMeasure", "_": format!("{value}"), "uom": uom },
            "MeasureClass": "length",
        }));
    };
    if let Some(length) = &hole.length {
        measure("boreholeLength", length.value, &length.uom);
    }
    if let Some((begin, end)) = &hole.drilled_interval {
        measure("drilledIntervalBegin", begin.value, &begin.uom);
        measure("drilledIntervalEnd", end.value, &end.uom);
    }
    if !hole.intervals.is_empty() {
        out.push(json!({
            "$type": "eml23.ExtensionNameValue",
            "Name": "sigeolIntervalCount",
            "Value": { "$type": "eml23.StringMeasure", "_": hole.intervals.len().to_string() },
            "Description": "the `logElement` intervals the measured depths were rebuilt from",
        }));
    }
    out
}
