//! `terrain` — a `Grid2dRepresentation` from a digital elevation model fetched
//! elsewhere, its **source, licence and attribution written with it**.
//!
//! The need: to see these DEM data in the 3D canvas,
//! to have zone textures with relief; and
//! to be able to manage catalogues for the inputs of this
//! terrain operation.
//!
//! # What this operation is, and is not
//!
//! The catalogue of terrain sources, the request and the raster reader live in
//! `energyml-providers`, which depends on this crate and not the other way
//! round. **The operation is pure**: no network in `plan`. The fetched raster
//! arrives as a [`Lattice`] piece already in the house order (origin = the
//! south-west node, `z[j·ni + i]`, `j` along +y, `NaN` where the source has no
//! data), and what the source *is* arrives as parameters. The writing is
//! `grid::write`'s: the same objects, the same data file, the same refusals
//! (spacing, count of heights, a frame that would bend the axes).
//!
//! # Why `Category::Create`
//!
//! `Create` reads *a representation made from a shape or a file, copied as it
//! is* — which is what this does to a raster that somebody else measured. Not
//! `Analysis`, which is *what is measured on a shape and written as a
//! property* (and a test forbids a representation under it), not `Geometry`,
//! which decides a new geometry from the input: nothing is decided here, the
//! lattice is written as it came. `Import` is for a foreign **vocabulary**
//! (SIGEOL, GeoSciML); a raster has none.
//!
//! # What travels with the data
//!
//! A terrain is somebody's measurement under somebody's licence. The activity
//! records `source`, `service`, `layer`, `licence`, `attribution` and `fetched`
//! (the instant, **given**: an operation reads no clock); the same licence and
//! attribution go into the object's `Citation/Description`, so that they stay
//! with the grid when the activity is not read; and the plan says them in a
//! note. All six are **refused when empty** — a terrain whose origin cannot be
//! named is a surface that looks like one and cannot be reused.
//!
//! # Refused by name
//!
//! - fewer than 2 × 2 nodes: a point or a line is not a surface;
//! - a lattice **all** of whose nodes are `NaN`: *a DEM with no value is not a
//!   surface*. A partial hole is written and counted (`grid::write` says
//!   `holes`), the absence of any value is a refusal;
//! - an empty `source`, `service`, `layer`, `licence`, `attribution` or
//!   `fetched`, or a `fetched` that is not an ISO date or instant.
//!
//! # Identity
//!
//! The seed is the content: the source, the layer, the lattice (origin,
//! spacing, counts) and the title — **never the clock, and not `fetched`**.
//! The same fetch planned twice writes the same uuid; a refetch of the same
//! tile a month later is the same terrain, and its activity says when.

use energyml_domain::provenance::Parameter;
use serde_json::Value;

use crate::declare::{Inputs, LatticeIn, Params, Suggestions, Title};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::support;
use super::written::Written;

/// The operation.
pub struct Terrain;

/// What `terrain` is told: where the elevations come from, and the shared
/// block of every writer.
#[derive(Debug, Params)]
pub struct TerrainParams {
    /// the catalogue id of the source the elevations come from — `ign-rge-alti`
    pub source: String,
    /// the service that was asked — the URL of the request, or of the endpoint
    pub service: String,
    /// the layer or the dataset asked of that service
    pub layer: String,
    /// the licence the data is under — it travels with the grid
    pub licence: String,
    /// the attribution the licence asks to display — it travels with the grid
    pub attribution: String,
    /// when the data was fetched, as an ISO 8601 instant — given, never read from a clock
    pub fetched: String,
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// What `terrain` takes.
#[derive(Debug, Inputs)]
pub struct TerrainInputs {
    /// the digital elevation model — a lattice of heights in the house order, `NaN` where the source has none
    pub lattice: LatticeIn,
}

fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double { title: title.to_string(), value, uom: None }
}

fn text(title: &str, value: &str) -> Parameter {
    Parameter::Text { title: title.to_string(), value: value.to_string() }
}

/// A source field that says nothing is refused by its name.
fn given(slot: &str, value: &str, what: &str) -> Result<(), Refusal> {
    if value.trim().is_empty() {
        return Err(Refusal::on(slot, format!("is empty: {what} is written with the terrain and cannot be left unsaid")));
    }
    Ok(())
}

/// `2026-10-01` or `2026-10-01T12:00:00Z`: four digits, a dash, and — beyond
/// the date — a `T`. Not a parser: what is refused is a word that is no date.
fn looks_like_iso(value: &str) -> bool {
    let bytes = value.trim().as_bytes();
    bytes.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && (bytes.len() == 10 || bytes[10] == b'T' || bytes[10] == b't')
}

impl Operation for Terrain {
    type Params = TerrainParams;
    type Inputs = TerrainInputs;

    fn about(&self) -> About {
        About {
            name: "terrain",
            describe: "a terrain — a 2D grid of ground elevations from a digital elevation model fetched elsewhere, with its source, licence and attribution written beside the heights",
            category: Category::Create,
            makes: &["resqml22.Grid2dRepresentation"],
            recipe: "terrain",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        // A suggestion reads the bag, never another answer: the title built
        // from the source (`MNT — <source>`) is derived in `plan`, where the
        // source is known — here the piece's name stands in for it.
        super::written::suggest_written(bag, None, slots);
    }

    fn plan(&self, inputs: TerrainInputs, params: TerrainParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let TerrainParams { source, service, layer, licence, attribution, fetched, mut written } = params;
        given("source", &source, "where the elevations come from")?;
        given("service", &service, "the service that was asked")?;
        given("layer", &layer, "the layer that was asked")?;
        given("licence", &licence, "the licence the data is under")?;
        given("attribution", &attribution, "the attribution the licence asks for")?;
        given("fetched", &fetched, "when the data was fetched")?;
        if !looks_like_iso(&fetched) {
            return Err(Refusal::on("fetched", format!("`{fetched}` is no ISO 8601 date or instant (2026-10-01 or 2026-10-01T12:00:00Z)")));
        }

        let name = &inputs.lattice.name;
        let lattice = &inputs.lattice.lattice;
        let [ni, nj] = lattice.counts;
        if ni < 2 || nj < 2 {
            return Err(Refusal::on(
                "lattice",
                format!("`{name}` is {ni} × {nj} nodes: a terrain is a surface and needs at least 2 nodes along each axis"),
            ));
        }
        // Left to `grid::write` when the heights do not match the counts: its
        // refusal names that, and "all NaN" of an empty list would not.
        if !lattice.z.is_empty() && lattice.z.iter().all(|z| !z.is_finite()) {
            return Err(Refusal::on(
                "lattice",
                format!("all {} node(s) of `{name}` have no height: a DEM with no value is not a surface — the source covers nothing of the area asked", lattice.z.len()),
            ));
        }
        cx.stop_if_cancelled()?;

        // A title left to the suggestion (the piece's name) or to nobody is
        // derived from the source, which only `plan` knows.
        let left_to_default = written.naming.title.as_ref().is_none_or(|title| title.0 == *name);
        if left_to_default {
            written.naming.title = Some(Title(format!("MNT — {source}")));
        }
        let title = written.naming.title();

        let [x0, y0] = lattice.origin;
        let [dx, dy] = lattice.spacing;
        let seed = format!("terrain:{source}:{layer}:{x0}:{y0}:{dx}:{dy}:{ni}x{nj}:{title}");
        let mut plan = super::grid::write(name, lattice, &written, &seed, cx)?;

        let description = format!("Terrain from `{source}` ({service}), layer `{layer}`. Licence: {licence}. Attribution: {attribution}. Fetched {fetched}.");
        let length = description.chars().count();
        if length > 2000 {
            return Err(Refusal::on(
                "licence",
                format!("source, service, layer, licence and attribution make a description of {length} characters, and a Citation/Description holds 2000 — say them shorter"),
            ));
        }
        if let Some(object) = plan.objects.iter_mut().rfind(|one| one.qualified_type == "resqml22.Grid2dRepresentation") {
            object.json["Citation"]["Description"] = Value::String(description);
        }

        let holes = lattice.z.iter().filter(|z| !z.is_finite()).count();
        plan.activity.recipe = "terrain";
        plan.activity.title = format!("terrain → {title}");
        plan.activity.parameters.extend([
            text("source", &source),
            text("service", &service),
            text("layer", &layer),
            text("licence", &licence),
            text("attribution", &attribution),
            text("fetched", &fetched),
            double("originX", x0),
            double("originY", y0),
            double("spacing", dx),
            double("spacingY", dy),
            double("holeCount", holes as f64),
        ]);
        // First, ahead of the frame's own notes: the attribution is the one
        // thing a reader of this plan must not miss.
        plan.says.insert(
            0,
            Note {
                key: "source",
                detail: format!(
                    "elevations of `{source}` ({service}, layer `{layer}`) fetched {fetched}, under the licence {licence} — attribution to display: {attribution}"
                ),
            },
        );
        plan.says.push(Note {
            key: "lattice",
            detail: format!("{ni} by {nj} nodes from ({x0}, {y0}) every ({dx}, {dy}), written as fetched — not resampled"),
        });
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| Slot::required(name, Need::Number { uom: None, least: None, most: None }, about);
        support::frame_reports(vec![
            count("nodeCount", "nodes written"),
            count("fastestAxisCount", "nodes along x"),
            count("slowestAxisCount", "nodes along y"),
            count("originX", "the x of the node (0, 0)"),
            count("originY", "the y of the node (0, 0)"),
            count("spacing", "the step along x"),
            count("spacingY", "the step along y"),
            count("holeCount", "nodes with no height, written as NaN"),
        ])
    }
}
