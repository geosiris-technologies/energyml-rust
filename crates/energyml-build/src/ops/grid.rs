//! `grid` — a `Grid2dRepresentation` from a regular lattice of heights.
//!
//! Written the way the corpus writes horizons: an x, y lattice as the
//! supporting geometry and the z values in an external array, the **slow
//! axis first** (the house lattice rule — see `Lattice`). The lattice is
//! written as it is, in the frame it was computed in: reprojecting one
//! would bend its axes, so a frame created here has to be the lattice's own
//! system, and a frame of the package is taken on its word.

use serde_json::{json, Value};

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, LatticeIn, Params, Suggestions};
use crate::operation::{About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan, Refusal, Slot};
use crate::pieces::{Lattice, Pieces};

use super::support;
use super::written::Written;

/// The operation.
pub struct Grid;

/// What `grid` is told: the shared block of every writer.
#[derive(Debug, Params)]
pub struct GridParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What `grid` takes.
#[derive(Debug, Inputs)]
pub struct GridInputs {
    /// the lattice of heights to write — one grid per plan
    pub lattice: LatticeIn,
}

impl Operation for Grid {
    type Params = GridParams;
    type Inputs = GridInputs;

    fn about(&self) -> About {
        About {
            name: "grid",
            describe: "a 2D grid — a map — from a regular lattice of heights: the x, y lattice as supporting geometry, the z values in the data file",
            category: Category::Create,
            makes: &["resqml22.Grid2dRepresentation"],
            recipe: "grid",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn plan(&self, inputs: GridInputs, params: GridParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let seed = cx.seed(&params.written.naming.title());
        write(&inputs.lattice.name, &inputs.lattice.lattice, &params.written, &seed, cx)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(name, Need::Number { uom: None, least: None, most: None }, about)
        };
        support::frame_reports(vec![
            count("nodeCount", "nodes written"),
            count("fastestAxisCount", "nodes along x"),
            count("slowestAxisCount", "nodes along y"),
        ])
    }
}

/// **The grid, written** — what the operation does, and what an operation
/// that computes a lattice (`interpolate`) calls directly: typed in, no bag
/// to derive and no choices to read back by name.
///
/// `name` is what the lattice is called in a refusal, `seed` what the derived
/// identities hang from — the caller's, so that two routes to the same grid
/// name it the same.
pub fn write(name: &str, lattice: &Lattice, written: &Written, seed: &str, cx: &Context<'_>) -> Result<Plan, Refusal> {
    let [ni, nj] = lattice.counts;
    let [dx, dy] = lattice.spacing;
    let [x0, y0] = lattice.origin;
    if ni < 2 || nj < 2 {
        return Err(Refusal::on("lattice", format!("`{name}`: a grid needs at least 2 nodes along each axis — {ni} × {nj} given")));
    }
    if !(dx.is_finite() && dx > 0.0 && dy.is_finite() && dy > 0.0) {
        return Err(Refusal::on("lattice", format!("`{name}`: the spacing must be positive — [{dx}, {dy}] given")));
    }
    let nodes = ni * nj;
    if lattice.z.len() != nodes {
        return Err(Refusal::on(
            "lattice",
            format!("`{name}`: {ni} × {nj} nodes take {nodes} height(s) and {} were given", lattice.z.len()),
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

        let dimension = |direction: [f64; 3], spacing: f64, count: usize| -> Value {
            json!({
                "$type": "resqml22.Point3dLatticeDimension",
                "Direction": {
                    "$type": "resqml22.Point3d",
                    "Coordinate1": direction[0],
                    "Coordinate2": direction[1],
                    "Coordinate3": direction[2],
                },
                "Spacing": {
                    "$type": "eml23.FloatingPointConstantArray",
                    "Value": spacing,
                    "Count": count,
                },
            })
        };
        let mut object = json!({
            "$type": "resqml22.Grid2dRepresentation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &title),
            "SurfaceRole": "map",
            "FastestAxisCount": ni,
            "SlowestAxisCount": nj,
            "Geometry": {
                "$type": "resqml22.PointGeometry",
                "LocalCrs": frame.crs,
                "Points": {
                    "$type": "resqml22.Point3dZValueArray",
                    "SupportingGeometry": {
                        "$type": "resqml22.Point3dLatticeArray",
                        "AllDimensionsAreOrthogonal": true,
                        "Origin": {
                            "$type": "resqml22.Point3d",
                            "Coordinate1": x0,
                            "Coordinate2": y0,
                            "Coordinate3": 0.0,
                        },
                        // The slow axis (j, along y) first: the last dimension
                        // varies fastest, as the array beside it is laid out.
                        "Dimension": [
                            dimension([0.0, 1.0, 0.0], dy, nj - 1),
                            dimension([1.0, 0.0, 0.0], dx, ni - 1),
                        ],
                    },
                    "ZValues": {
                        "$type": "eml23.FloatingPointExternalArray",
                        "Values": support::external(&file, &path, &[nj as i64, ni as i64]),
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

        // **The computed shape**: the lattice expanded into points,
        // and the topology that says it is one.
        //
        // # Why the expansion is here and not borrowed from the reader
        //
        // `energyml_geometry::points::lattice` reads a lattice that was **written**,
        // through reflection, and handles what producers write: two
        // spellings, variable steps, the SISMAGE convention where the direction
        // carries the axis extent. None of that is at stake here: this
        // operation **writes** the lattice two lines above, with
        // unit directions and a constant step, and this is its immediate
        // inverse.
        //
        // What keeps the two in agreement is therefore not shared code — the
        // boundary is a reflection boundary — but a check:
        // `a_written_lattice_reads_back_as_the_shape_it_carried` applies the
        // plan, reads the object back through the real reader, and compares point by point.
        let points: Vec<[f64; 3]> = (0..nj)
            .flat_map(|j| (0..ni).map(move |i| (i, j)))
            .map(|(i, j)| {
                [x0 + i as f64 * dx, y0 + j as f64 * dy, lattice.z.get(j * ni + i).copied().unwrap_or(f64::NAN)]
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
                    topology: energyml_geometry::geometry::Topology::Grid { fastest: ni, slowest: nj },
                    closed: Vec::new(),
                }],
            },
        );

        let holes = lattice.z.iter().filter(|z| !z.is_finite()).count();
        let mut says = if holes > 0 {
            vec![Note { key: "holes", detail: format!("{holes} of {nodes} node(s) have no height and are written as NaN") }]
        } else {
            Vec::new()
        };
        // What the frame must say of itself, after what the grid said.
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
        parameters.push(Parameter::Double { title: "nodeCount".to_string(), value: nodes as f64, uom: None });
        parameters.push(Parameter::Double { title: "fastestAxisCount".to_string(), value: ni as f64, uom: None });
        parameters.push(Parameter::Double { title: "slowestAxisCount".to_string(), value: nj as f64, uom: None });
        Ok(Plan {
            media: Vec::new(),
            // What the frame flagged travels up to here: the geometry is
            // computed, the writing is what is missing. `apply` refuses.
            blocked: frame.blocked.clone(),
            objects,
            datasets: vec![Dataset {
                file,
                path,
                dimensions: vec![nj as i64, ni as i64],
                values: Numbers::F64(lattice.z.clone()),
            }],
            shapes: vec![shape],
            activity: ActivityDraft { recipe: "grid", title: format!("grid → {title}"), seed, parameters },
            says,
            dropped: Vec::new(),
        })
}
