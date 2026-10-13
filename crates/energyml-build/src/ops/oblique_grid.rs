//! A `Grid2dRepresentation` from an origin and two direction vectors, each
//! with its own step and count — two direction vectors, a scale for each,
//! a number of i and j, and a starting point.
//!
//! # Why this is a separate writer, not a change to `grid::write`
//!
//! `crates/energyml-build/src/ops/grid.rs::write` hardcodes the two axes at
//! `[1, 0, 0]` and `[0, 1, 0]` and always writes
//! `AllDimensionsAreOrthogonal: true` — the case every method wrote until
//! now. The idea is extending that one function so every method benefits (the extension
//! serves every method, not only MBA), which is real and
//! worth doing, but is a change to an existing file's behaviour, not an
//! addition — and this crate's rule for the task at hand is the other way
//! round: only new files, every retouch to an existing one written out for
//! the maintainer to apply.
//! This file is the addition that lets `interpolate`'s `Method::Mba` with
//! `extent = given` (`crates/energyml-build/src/ops/interpolate_mba.rs`)
//! write something *today*; folding it back into `grid::write` — so every
//! method gains oblique lattices, not only MBA — is the natural next step,
//! named here rather than done silently.
//!
//! It is `write`'s own body with the two directions and the orthogonality
//! flag as parameters instead of literals — same frame handling, same
//! citation, same activity and holes bookkeeping, same computed shape for
//! the scene (the shape is the output). Nothing here decides *whether* the
//! two directions are orthogonal: `write_oblique`'s caller states it, having
//! computed it or decided to leave it out (the business
//! rule: *"If you don't know it or if only one lattice dimension is given,
//! do not provide this element"*, RESQML TRG 2.2 §3.17 p. 40).

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;

use crate::operation::{Context, Dataset, NewObject, Note, Numbers, Plan, Refusal};

use super::support;
use super::written::Written;

/// The grid, written with an oblique — or simply a second, not-yet-checked —
/// pair of directions. `direction1`/`direction2` are unit vectors in the
/// plan; `z` is `count1 × count2` heights, the first (`direction1`) axis
/// varying fastest — the same order [`super::grid::write`] uses for its own
/// `[ni, nj]`. `orthogonal` is written as given, `AllDimensionsAreOrthogonal`
/// left out on `None` (the business rule cited in the module doc).
#[allow(clippy::too_many_arguments)]
pub fn write(
    name: &str,
    origin: [f64; 2],
    direction1: [f64; 2],
    step1: f64,
    count1: usize,
    direction2: [f64; 2],
    step2: f64,
    count2: usize,
    orthogonal: Option<bool>,
    z: &[f64],
    written: &Written,
    seed: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    if count1 < 2 || count2 < 2 {
        return Err(Refusal::on(
            "count1",
            format!("`{name}`: a grid needs at least 2 nodes along each axis — {count1} × {count2} given"),
        ));
    }
    if !(step1.is_finite() && step1 > 0.0 && step2.is_finite() && step2 > 0.0) {
        return Err(Refusal::on(
            "step1",
            format!("`{name}`: the steps must be positive — [{step1}, {step2}] given"),
        ));
    }
    let nodes = count1 * count2;
    if z.len() != nodes {
        return Err(Refusal::on(
            "shape",
            format!(
                "`{name}`: {count1} × {count2} nodes take {nodes} height(s) and {} were given",
                z.len()
            ),
        ));
    }

    let title = written.naming.title();
    let seed = seed.to_string();
    let mut rank = 0usize;
    let frame = support::frame_in(&cx.known(), &written.frame, cx.identity, cx.when, &mut rank)?;
    if let Some((target, source)) = &frame.place
        && target.epsg() != *source
    {
        return Err(Refusal::on(
            "project",
            format!(
                "a lattice is written as it is, in the frame it was computed in: placing EPSG:{source} into EPSG:{} would bend its axes — name a `crs` of the package, or create the frame with `sourceEpsg` equal to `project`",
                target.epsg()
            ),
        ));
    }
    let uuid = support::next_uuid(cx.identity, &format!("grid:{seed}"), &mut rank)?;
    let file = written.naming.data_file.clone();
    let path = format!("/resqml22/{uuid}/z");

    let dimension = |direction: [f64; 2], spacing: f64, count: usize| -> Value {
        json!({
            "$type": "resqml22.Point3dLatticeDimension",
            "Direction": {
                "$type": "resqml22.Point3d",
                "Coordinate1": direction[0],
                "Coordinate2": direction[1],
                "Coordinate3": 0.0,
            },
            "Spacing": {
                "$type": "eml23.FloatingPointConstantArray",
                "Value": spacing,
                "Count": count,
            },
        })
    };
    let mut lattice = json!({
        "$type": "resqml22.Point3dLatticeArray",
        "Origin": {
            "$type": "resqml22.Point3d",
            "Coordinate1": origin[0],
            "Coordinate2": origin[1],
            "Coordinate3": 0.0,
        },
        // The slow axis (`direction2`) first, the fast one
        // (`direction1`) second — the same order `grid::write` already
        // uses for its own [0,1,0]/[1,0,0], the last dimension varying
        // fastest.
        "Dimension": [
            dimension(direction2, step2, count2 - 1),
            dimension(direction1, step1, count1 - 1),
        ],
    });
    if let Some(orthogonal) = orthogonal {
        lattice["AllDimensionsAreOrthogonal"] = json!(orthogonal);
    }
    let mut object = json!({
        "$type": "resqml22.Grid2dRepresentation",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        "SurfaceRole": "map",
        "FastestAxisCount": count1,
        "SlowestAxisCount": count2,
        "Geometry": {
            "$type": "resqml22.PointGeometry",
            "LocalCrs": frame.crs,
            "Points": {
                "$type": "resqml22.Point3dZValueArray",
                "SupportingGeometry": lattice,
                "ZValues": {
                    "$type": "eml23.FloatingPointExternalArray",
                    "Values": support::external(&file, &path, &[count2 as i64, count1 as i64]),
                },
            },
        },
    });
    let represented = support::represented_in(
        &written.represented,
        Some(&written.new_feature),
        None,
        cx.identity,
        cx.when,
        &seed,
        &title,
        &mut rank,
    )?;
    if let Some(reference) = represented.reference {
        object["RepresentedObject"] = reference;
    }

    // The lattice developed into points — same reasoning as `grid::write`'s
    // own: this operation writes the lattice with
    // whatever direction vectors it was given, and unrolling it here is the
    // inverse of that, not a borrow from the reflective reader.
    let points: Vec<[f64; 3]> = (0..count2)
        .flat_map(|j| (0..count1).map(move |i| (i, j)))
        .map(|(i, j)| {
            [
                origin[0] + i as f64 * step1 * direction1[0] + j as f64 * step2 * direction2[0],
                origin[1] + i as f64 * step1 * direction1[1] + j as f64 * step2 * direction2[1],
                z.get(j * count1 + i).copied().unwrap_or(f64::NAN),
            ]
        })
        .collect();
    let shape = support::made_root(
        &title,
        &uuid,
        energyml_geometry::geometry::Geometry {
            kind: "grid",
            patches: vec![energyml_geometry::geometry::Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: energyml_geometry::geometry::Topology::Grid {
                    fastest: count1,
                    slowest: count2,
                },
                closed: Vec::new(),
            }],
        },
    );

    let holes = z.iter().filter(|v| !v.is_finite()).count();
    let mut says = if holes > 0 {
        vec![Note {
            key: "holes",
            detail: format!("{holes} of {nodes} node(s) have no height and are written as NaN"),
        }]
    } else {
        Vec::new()
    };
    says.extend(frame.says);

    let mut objects = frame.objects;
    objects.extend(represented.objects);
    objects.push(NewObject {
        uuid,
        qualified_type: "resqml22.Grid2dRepresentation".to_string(),
        role: "representation".to_string(),
        title: title.clone(),
        json: object,
    });
    let mut parameters = frame.parameters;
    parameters.push(Parameter::Double {
        title: "nodeCount".to_string(),
        value: nodes as f64,
        uom: None,
    });
    parameters.push(Parameter::Double {
        title: "fastestAxisCount".to_string(),
        value: count1 as f64,
        uom: None,
    });
    parameters.push(Parameter::Double {
        title: "slowestAxisCount".to_string(),
        value: count2 as f64,
        uom: None,
    });
    if let Some(orthogonal) = orthogonal {
        parameters.push(Parameter::Text {
            title: "allDimensionsAreOrthogonal".to_string(),
            value: orthogonal.to_string(),
        });
    }
    Ok(Plan {
        media: Vec::new(),
        blocked: frame.blocked.clone(),
        objects,
        datasets: vec![Dataset {
            file,
            path,
            dimensions: vec![count2 as i64, count1 as i64],
            values: Numbers::F64(z.to_vec()),
        }],
        shapes: vec![shape],
        activity: crate::operation::ActivityDraft {
            recipe: "interpolate",
            title: format!("interpolate → {title}"),
            seed,
            parameters,
        },
        says,
        dropped: Vec::new(),
    })
}
