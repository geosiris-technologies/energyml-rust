//! `terrain` — a grid of ground elevations from a DEM fetched elsewhere. What
//! the writing of the grid does (the hole count, the frame, the data file) is
//! `grid`'s own; this holds what the terrain adds: **the source, the licence
//! and the attribution travel with the heights**, and a DEM that covers
//! nothing is refused.
//!
//! The lattice is fabricated — 4 × 3 nodes, a step of 25 along x and 12.5
//! along y, `z = 100 + i + 10 j` with one hole — so that a transposed order, a
//! swapped step or a lost hole each shows. No network, no file: the raster
//! reaches the operation as a piece, as `energyml-providers` will hand it.
//!
//! # What would have to break to make it turn red
//!
//! Writing the grid from a lattice other than that of the input (origin,
//! steps or counts), losing or swapping `source`, `service`, `layer`,
//! `licence`, `attribution`, `fetched` in the activity, keeping quiet about the licence in
//! the note or in the citation, letting through a DEM without a single value,
//! a row or a column of nodes, an empty source, or reading the clock — or
//! `fetched` — in the seed of the identity.

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Lattice, Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

fn when() -> When {
    When {
        creation: "2026-10-01T00:00:00Z".into(),
        originator: "test".into(),
    }
}

const SOURCE: &str = "ign-rge-alti";
const SERVICE: &str = "https://data.geopf.fr/wms-r";
const LAYER: &str = "ELEVATION.ELEVATIONGRIDCOVERAGE";
const LICENCE: &str = "Licence Ouverte Etalab 2.0";
const ATTRIBUTION: &str = "IGN — RGE ALTI";
const FETCHED: &str = "2026-10-01T08:30:00Z";

const ORIGIN: [f64; 2] = [652_000.0, 6_862_000.0];
const STEPS: [f64; 2] = [25.0, 12.5];

/// 4 × 3 nodes, `z[j·4 + i] = 100 + i + 10 j`, the node `(2, 1)` a hole.
fn dem() -> Lattice {
    let z = (0..3)
        .flat_map(|j| {
            (0..4).map(move |i| {
                if (i, j) == (2, 1) {
                    f64::NAN
                } else {
                    100.0 + i as f64 + 10.0 * j as f64
                }
            })
        })
        .collect();
    Lattice {
        origin: ORIGIN,
        spacing: STEPS,
        counts: [4, 3],
        z,
    }
}

fn bag(lattice: Lattice) -> Pieces {
    Pieces::empty().with("lattice", Source::Lattice(lattice))
}

fn answers() -> Choices {
    Choices::new()
        .set("source", SOURCE)
        .set("service", SERVICE)
        .set("layer", LAYER)
        .set("licence", LICENCE)
        .set("attribution", ATTRIBUTION)
        .set("fetched", FETCHED)
        .set("title", "MNT test")
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("vertical", 5720)
}

fn plan(bag: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("terrain", bag, choices, &Identity::derived(), &when())
        .map_err(|why| why.message())
}

fn grid_of(plan: &Plan) -> &energyml_build::operation::NewObject {
    plan.objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.Grid2dRepresentation")
        .expect("a Grid2dRepresentation")
}

fn text_of(plan: &Plan, title: &str) -> Option<String> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Text { title: t, value } if t == title => Some(value.clone()),
        _ => None,
    })
}

fn number_of(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title: t, value, .. } if t == title => Some(*value),
        _ => None,
    })
}

/// **One grid, on the lattice that was given** — the node count, the origin
/// and the steps are the DEM's own, the heights keep their order and the hole
/// stays a hole.
///
/// Casser : in `terrain::plan`, hand `grid::write` a lattice with `spacing`
/// swapped or `counts` reversed; or write `z` transposed.
#[test]
fn a_dem_is_written_as_it_came() {
    let made = plan(&bag(dem()), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let grids = made
        .objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.Grid2dRepresentation")
        .count();
    assert_eq!(grids, 1, "one grid per plan");

    let json = &grid_of(&made).json;
    assert_eq!(json["FastestAxisCount"], 4);
    assert_eq!(json["SlowestAxisCount"], 3);
    let lattice = &json["Geometry"]["Points"]["SupportingGeometry"];
    assert_eq!(lattice["Origin"]["Coordinate1"], ORIGIN[0]);
    assert_eq!(lattice["Origin"]["Coordinate2"], ORIGIN[1]);
    // The slow axis (y) first, then x: the house order.
    assert_eq!(lattice["Dimension"][0]["Spacing"]["Value"], STEPS[1]);
    assert_eq!(lattice["Dimension"][0]["Spacing"]["Count"], 2);
    assert_eq!(lattice["Dimension"][1]["Spacing"]["Value"], STEPS[0]);
    assert_eq!(lattice["Dimension"][1]["Spacing"]["Count"], 3);

    let dataset = &made.datasets[0];
    assert_eq!(dataset.dimensions, vec![3, 4], "the array is [nj, ni]");
    let Numbers::F64(values) = &dataset.values else {
        panic!("heights are floats")
    };
    let want = dem().z;
    assert_eq!(values.len(), want.len());
    for (at, (got, want)) in values.iter().zip(&want).enumerate() {
        assert!(
            got == want || (got.is_nan() && want.is_nan()),
            "node {at}: {got} for {want}"
        );
    }
    assert!(values[6].is_nan(), "the hole stays where it was");

    // The computed shape agrees with the lattice, point by point.
    let patch = &made.shapes[0].geometry.patches[0];
    assert_eq!(
        patch.topology,
        Topology::Grid {
            fastest: 4,
            slowest: 3
        }
    );
    assert_eq!(patch.points.len(), 12);
    assert_eq!(patch.points[0][..2], ORIGIN);
    assert_eq!(patch.points[1][0] - patch.points[0][0], STEPS[0]);
    assert_eq!(patch.points[4][1] - patch.points[0][1], STEPS[1]);
}

/// **The activity says where the heights come from** — and what the lattice
/// was, as `interpolate` says its own.
///
/// Casser : drop one of the six `text(…)` pushes in `terrain::plan` *and* the
/// registry's recording of choices (a slot renamed in `TerrainParams`); or
/// push `licence` under `attribution`.
#[test]
fn the_activity_says_where_the_heights_come_from() {
    let made = plan(&bag(dem()), &answers()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(made.activity.recipe, "terrain");
    assert_eq!(text_of(&made, "source").as_deref(), Some(SOURCE));
    assert_eq!(text_of(&made, "service").as_deref(), Some(SERVICE));
    assert_eq!(text_of(&made, "layer").as_deref(), Some(LAYER));
    assert_eq!(text_of(&made, "licence").as_deref(), Some(LICENCE));
    assert_eq!(text_of(&made, "attribution").as_deref(), Some(ATTRIBUTION));
    assert_eq!(text_of(&made, "fetched").as_deref(), Some(FETCHED));
    // Once each: the registry does not write again what the operation wrote.
    for title in ["source", "licence", "fetched"] {
        let times = made
            .activity
            .parameters
            .iter()
            .filter(|p| matches!(p, Parameter::Text { title: t, .. } if t == title))
            .count();
        assert_eq!(times, 1, "`{title}` is recorded once");
    }
    assert_eq!(number_of(&made, "originX"), Some(ORIGIN[0]));
    assert_eq!(number_of(&made, "originY"), Some(ORIGIN[1]));
    assert_eq!(number_of(&made, "spacing"), Some(STEPS[0]));
    assert_eq!(number_of(&made, "spacingY"), Some(STEPS[1]));
    assert_eq!(number_of(&made, "fastestAxisCount"), Some(4.0));
    assert_eq!(number_of(&made, "slowestAxisCount"), Some(3.0));
    assert_eq!(number_of(&made, "holeCount"), Some(1.0));
}

/// **The licence and the attribution travel with the data**: in the note the
/// plan says, and in the grid's own citation.
///
/// Casser : remove the `source` note, or the `Citation/Description` write in
/// `terrain::plan`.
#[test]
fn the_licence_travels_with_the_data() {
    let made = plan(&bag(dem()), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let note = made
        .says
        .iter()
        .find(|note| note.key == "source")
        .expect("a note naming the source");
    assert!(
        note.detail.contains(SOURCE) && note.detail.contains(LICENCE) && note.detail.contains(ATTRIBUTION),
        "{}",
        note.detail
    );
    assert_eq!(
        made.says.first().map(|note| note.key),
        Some("source"),
        "the attribution comes first"
    );

    let description = grid_of(&made).json["Citation"]["Description"]
        .as_str()
        .expect("a description")
        .to_string();
    assert!(
        description.contains(LICENCE) && description.contains(ATTRIBUTION) && description.contains(SOURCE),
        "{description}"
    );
    assert_eq!(grid_of(&made).json["Citation"]["Title"], "MNT test");
}

/// **A title left out is derived from the source**, not from the clock.
///
/// Casser : drop the `left_to_default` branch of `terrain::plan`.
#[test]
fn the_title_comes_from_the_source() {
    // The suggestion is the piece's name; `plan` replaces it by the source's.
    let choices = answers().set("title", "lattice");
    let made = plan(&bag(dem()), &choices).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        grid_of(&made).json["Citation"]["Title"],
        format!("MNT — {SOURCE}")
    );
}

/// **A DEM with no value is not a surface**, and a point or a line is not one
/// either — each refused by name, by this operation and not by the writer
/// beneath it.
///
/// Casser : remove the all-NaN refusal (the plan would write a grid of holes),
/// or the `ni < 2 || nj < 2` one (`grid::write` would still refuse, with its
/// own words — which the assertion tells apart).
#[test]
fn a_dem_without_value_or_without_surface_is_refused() {
    let mut empty = dem();
    empty.z = vec![f64::NAN; 12];
    let why = plan(&bag(empty), &answers()).expect_err("no value anywhere");
    assert!(why.contains("a DEM with no value is not a surface"), "{why}");

    let one_column = Lattice {
        origin: ORIGIN,
        spacing: STEPS,
        counts: [1, 5],
        z: vec![1.0, 2.0, 3.0, 4.0, 5.0],
    };
    let why = plan(&bag(one_column), &answers()).expect_err("one column");
    assert!(why.contains("a terrain is a surface"), "{why}");
    let one_row = Lattice {
        origin: ORIGIN,
        spacing: STEPS,
        counts: [5, 1],
        z: vec![1.0, 2.0, 3.0, 4.0, 5.0],
    };
    let why = plan(&bag(one_row), &answers()).expect_err("one row");
    assert!(why.contains("a terrain is a surface"), "{why}");

    // One value is enough: a hole is counted, not refused.
    let mut sparse = dem();
    sparse.z = (0..12).map(|k| if k == 7 { 321.0 } else { f64::NAN }).collect();
    let made = plan(&bag(sparse), &answers()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(number_of(&made, "holeCount"), Some(11.0));
}

/// **A source that says nothing is refused by its name** — each of the six.
///
/// Casser : drop one `given(…)` call in `terrain::plan`; the loop names which.
#[test]
fn a_mute_source_is_refused_by_name() {
    for slot in ["source", "service", "layer", "licence", "attribution", "fetched"] {
        let why = plan(&bag(dem()), &answers().set(slot, "  ")).expect_err(slot);
        assert!(
            why.contains(&format!("`{slot}`")) && why.contains("is empty"),
            "{slot}: {why}"
        );
    }
    let why = plan(&bag(dem()), &answers().set("fetched", "yesterday")).expect_err("not a date");
    assert!(why.contains("`fetched`") && why.contains("ISO 8601"), "{why}");
    // A date alone is an instant enough.
    plan(&bag(dem()), &answers().set("fetched", "2026-10-01")).unwrap_or_else(|why| panic!("{why}"));
}

/// **The same fetch writes the same uuid; another source writes another** —
/// and when it was fetched is not part of the name: the same tile refetched
/// is the same terrain.
///
/// Casser : put `fetched`, or a counter, in the seed of `terrain::plan` (the
/// first assertion); or leave `source` out of it (the second).
#[test]
fn identity_comes_from_the_content() {
    let first = plan(&bag(dem()), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let again = plan(&bag(dem()), &answers()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(grid_of(&first).uuid, grid_of(&again).uuid);

    let later = plan(&bag(dem()), &answers().set("fetched", "2026-11-15T10:00:00Z"))
        .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        grid_of(&first).uuid,
        grid_of(&later).uuid,
        "a refetch is the same terrain"
    );

    let other =
        plan(&bag(dem()), &answers().set("source", "swisstopo-alti3d")).unwrap_or_else(|why| panic!("{why}"));
    assert_ne!(
        grid_of(&first).uuid,
        grid_of(&other).uuid,
        "another source is another terrain"
    );

    let mut shifted = dem();
    shifted.origin = [ORIGIN[0] + 25.0, ORIGIN[1]];
    let moved = plan(&bag(shifted), &answers()).unwrap_or_else(|why| panic!("{why}"));
    assert_ne!(
        grid_of(&first).uuid,
        grid_of(&moved).uuid,
        "the same source on another tile is another terrain"
    );
}
