//! **The speed report of the operations** — `cargo bench --workspace --bench speed`.
//!
//! Not a test: nothing here fails, and neither `cargo test` nor the CI runs
//! it. What it measures is **the slope**, not the stopwatch — the time of an
//! operation at three or four sizes and the exponent drawn from them. The
//! absolute figure of a development machine moves from one day to the next; a
//! computation that quadruples when its input doubles shows on any of them.
//! The reasoning is that a slope is portable where a stopwatch is not; the report is
//! kept dated, with the machine it ran on.
//!
//! The inputs are **synthetic and deterministic** — surfaces, clouds and
//! fault traces drawn from a fixed seed — so two runs time the same work.
//! Each case is timed through `Registry::plan_for` (the whole plan, as the
//! front end asks it: `Purpose::Geometry`), and the two heaviest algorithms are
//! also timed alone, to say whether the time goes to the computation or to
//! the plan around it.
//!
//! `SPEED_ONLY=<word>` keeps the cases whose name holds it.

use std::time::{Duration, Instant};

use energyml_build::operation::{Choices, Identity, Purpose, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// The side of every synthetic extent, in metres.
const EXTENT: f64 = 10_000.0;

/// `splitmix64` — a fixed seed, the same numbers on every machine.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A smooth relief, so that contours, dips and curvatures have something to say.
fn relief(x: f64, y: f64) -> f64 {
    -1000.0 - 0.02 * x + 80.0 * (x / 1500.0).sin() * (y / 2100.0).cos()
}

fn patch(points: Vec<[f64; 3]>, topology: Topology) -> Geometry {
    Geometry {
        kind: "surface",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology,
            closed: Vec::new(),
        }],
    }
}

fn shape(geometry: Geometry) -> Source {
    Source::Shape {
        geometry,
        closed: None,
    }
}

/// A surface of about `nodes` nodes on a regular grid, two triangles a cell.
fn surface(nodes: usize, z: impl Fn(f64, f64) -> f64) -> Geometry {
    let side = (nodes as f64).sqrt().round().max(2.0) as u32;
    let step = EXTENT / f64::from(side - 1);
    let mut points = Vec::with_capacity((side * side) as usize);
    for j in 0..side {
        for i in 0..side {
            let (x, y) = (f64::from(i) * step, f64::from(j) * step);
            points.push([x, y, z(x, y)]);
        }
    }
    let mut faces = Vec::with_capacity((2 * (side - 1) * (side - 1)) as usize);
    for j in 0..side - 1 {
        for i in 0..side - 1 {
            let k = j * side + i;
            faces.push([k, k + 1, k + side + 1]);
            faces.push([k, k + side + 1, k + side]);
        }
    }
    patch(points, Topology::Triangles(faces))
}

/// `count` points scattered on the extent, on the relief.
fn cloud(count: usize, seed: u64) -> Geometry {
    let mut draw = Seeded(seed);
    let points = (0..count)
        .map(|_| {
            let (x, y) = (draw.next() * EXTENT, draw.next() * EXTENT);
            [x, y, relief(x, y)]
        })
        .collect();
    patch(points, Topology::Points)
}

/// `count` fault traces of `segments` segments each, crossing the extent.
fn traces(count: usize, segments: usize, seed: u64) -> Geometry {
    let mut draw = Seeded(seed);
    let mut points = Vec::new();
    for _ in 0..count {
        let (x0, y0) = (draw.next() * EXTENT, draw.next() * EXTENT * 0.2);
        let heading = 0.3 + draw.next() * 0.9;
        let length = EXTENT * 0.6 / segments as f64;
        for at in 0..=segments {
            let t = at as f64 * length;
            points.push([x0 + t * heading.cos() * 0.3, y0 + t * heading.sin(), -1000.0]);
        }
    }
    patch(points, Topology::Polylines(vec![segments as u32 + 1; count]))
}

/// A fault surface: a vertical sheet across the extent, `nodes` nodes, from
/// above the relief to below it.
fn fault_sheet(nodes: usize) -> Geometry {
    let side = (nodes as f64).sqrt().round().max(2.0) as u32;
    let mut sheet = surface(nodes, |_, _| 0.0);
    let step = 1.0 / f64::from(side - 1);
    for (at, point) in sheet.patches[0].points.iter_mut().enumerate() {
        let (i, j) = (at as u32 % side, at as u32 / side);
        let along = f64::from(i) * step * EXTENT;
        *point = [
            0.5 * EXTENT + 0.1 * along,
            along,
            -600.0 - f64::from(j) * step * 1200.0,
        ];
    }
    sheet
}

fn known(title: &str, nodes: usize) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(
            format!("{:08x}-0000-4000-8000-000000000000", title.len() * 7919 + nodes),
            "resqml22.TriangulatedSetRepresentation",
            title,
        )),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        node_count: Some(nodes),
        ..Default::default()
    })
}

fn nodes_of(geometry: &Geometry) -> usize {
    geometry.patches.iter().map(|patch| patch.points.len()).sum()
}

/// The spacing that makes a grid of about `nodes` nodes over the extent.
fn spacing_for(nodes: usize) -> f64 {
    EXTENT / ((nodes as f64).sqrt() - 1.0)
}

/// The median of a few passes — fewer the longer one takes.
fn median(mut run: impl FnMut()) -> Duration {
    let first = Instant::now();
    run();
    let first = first.elapsed();
    let passes = if first > Duration::from_secs(2) {
        return first;
    } else if first > Duration::from_millis(300) {
        3
    } else {
        7
    };
    let mut times: Vec<Duration> = (0..passes)
        .map(|_| {
            let at = Instant::now();
            run();
            at.elapsed()
        })
        .collect();
    times.sort();
    times[times.len() / 2]
}

/// One case: its name, what `n` counts, and a run for each size.
struct Case {
    name: &'static str,
    counts: &'static str,
    sizes: Vec<usize>,
    run: Box<dyn Fn(usize) -> Box<dyn FnMut()>>,
}

fn plan(name: &'static str, pieces: Pieces, choices: Choices) -> Box<dyn FnMut()> {
    let registry = Registry::new();
    let when = When {
        creation: "2026-09-12T00:00:00Z".into(),
        originator: "speed".into(),
    };
    Box::new(move || {
        let made = registry.plan_for(
            name,
            &pieces,
            &choices,
            &Identity::derived(),
            &when,
            Purpose::Geometry,
        );
        if let Err(why) = made {
            panic!("`{name}` refused: {why}");
        }
    })
}

fn cases() -> Vec<Case> {
    const SAMPLES: usize = 2000;
    vec![
        Case {
            name: "loft between two sticks — nodes per stick",
            counts: "nodes on each of two sticks",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                let stick = |x: f64, seed: f64| {
                    let points = (0..n).map(|k| [x, 0.1 * seed * k as f64, -(k as f64)]).collect();
                    shape(patch(points, Topology::Polylines(vec![n as u32])))
                };
                plan(
                    "loft",
                    Pieces::empty()
                        .with("lines", stick(0.0, 1.0))
                        .with("lines 2", stick(100.0, 2.0)),
                    Choices::new(),
                )
            }),
        },
        Case {
            name: "project onto a surface — nodes",
            counts: "nodes projected, onto 40 000 surface nodes",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                plan(
                    "project",
                    Pieces::empty().with("shape", shape(cloud(n, 6))).with(
                        "surface",
                        shape(surface(40_000, |x, y| -2000.0 + 0.01 * x - 0.02 * y)),
                    ),
                    Choices::new(),
                )
            }),
        },
        Case {
            name: "densify smooth — nodes added",
            counts: "nodes added, 10 traces × 50 segments",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                plan(
                    "densify",
                    Pieces::empty().with("shape", shape(traces(10, 50, 5))),
                    Choices::new().set("count", (n / 10) as i64).set("smooth", true),
                )
            }),
        },
        Case {
            name: "interpolate idw — grid nodes",
            counts: "grid nodes, 2 000 samples",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                plan(
                    "interpolate",
                    Pieces::empty().with("shape", shape(cloud(SAMPLES, 1))),
                    Choices::new().set("method", "idw").set("spacing", spacing_for(n)),
                )
            }),
        },
        Case {
            name: "interpolate idw — samples",
            counts: "samples, 40 000 grid nodes",
            sizes: vec![500, 2_000, 8_000],
            run: Box::new(|n| {
                plan(
                    "interpolate",
                    Pieces::empty().with("shape", shape(cloud(n, 2))),
                    Choices::new()
                        .set("method", "idw")
                        .set("spacing", spacing_for(40_000)),
                )
            }),
        },
        Case {
            name: "idw alone (energyml-analysis)",
            counts: "grid nodes, 2 000 samples",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                let samples: Vec<[f64; 2]> = cloud(SAMPLES, 1).patches[0]
                    .points
                    .iter()
                    .map(|p| [p[0], p[1]])
                    .collect();
                let values: Vec<f64> = cloud(SAMPLES, 1).patches[0].points.iter().map(|p| p[2]).collect();
                let queries: Vec<[f64; 2]> = surface(n, |_, _| 0.0).patches[0]
                    .points
                    .iter()
                    .map(|p| [p[0], p[1]])
                    .collect();
                Box::new(move || {
                    energyml_analysis::Idw::new(2.0)
                        .interpolate(&samples, &values, &queries)
                        .expect("idw");
                })
            }),
        },
        Case {
            name: "interpolate idw + faults",
            counts: "grid nodes, 2 000 samples, 10 traces × 50 segments",
            sizes: vec![2_500, 10_000, 40_000],
            run: Box::new(|n| {
                plan(
                    "interpolate",
                    Pieces::empty()
                        .with("shape", shape(cloud(SAMPLES, 3)))
                        .with("faults", shape(traces(10, 50, 4))),
                    Choices::new().set("method", "idw").set("spacing", spacing_for(n)),
                )
            }),
        },
        Case {
            name: "interpolate idw + faults, 32 neighbours",
            counts: "grid nodes, 2 000 samples, 10 traces × 50 segments",
            sizes: vec![2_500, 10_000, 40_000],
            run: Box::new(|n| {
                plan(
                    "interpolate",
                    Pieces::empty()
                        .with("shape", shape(cloud(SAMPLES, 3)))
                        .with("faults", shape(traces(10, 50, 4))),
                    Choices::new()
                        .set("method", "idw")
                        .set("neighbours", 32)
                        .set("spacing", spacing_for(n)),
                )
            }),
        },
        Case {
            name: "interpolate kriging — samples",
            counts: "samples, 10 000 grid nodes",
            sizes: vec![250, 500, 1_000],
            run: Box::new(|n| {
                plan(
                    "interpolate",
                    Pieces::empty().with("shape", shape(cloud(n, 5))),
                    Choices::new()
                        .set("method", "kriging")
                        .set("spacing", spacing_for(10_000)),
                )
            }),
        },
        Case {
            name: "interpolate kriging + faults",
            counts: "grid nodes, 500 samples, 10 traces × 50 segments, 32 neighbours",
            sizes: vec![2_500, 10_000, 40_000],
            run: Box::new(|n| {
                plan(
                    "interpolate",
                    Pieces::empty()
                        .with("shape", shape(cloud(500, 6)))
                        .with("faults", shape(traces(10, 50, 7))),
                    Choices::new()
                        .set("method", "kriging")
                        .set("neighbours", 32)
                        .set("spacing", spacing_for(n)),
                )
            }),
        },
        Case {
            name: "triangulate",
            counts: "points",
            sizes: vec![10_000, 100_000, 1_000_000],
            run: Box::new(|n| {
                plan(
                    "triangulate",
                    Pieces::empty().with("shape", shape(cloud(n, 8))),
                    Choices::new(),
                )
            }),
        },
        Case {
            name: "triangulate alone (energyml-analysis)",
            counts: "points",
            sizes: vec![10_000, 100_000, 1_000_000],
            run: Box::new(|n| {
                let flat: Vec<f64> = cloud(n, 8).patches[0].points.iter().flatten().copied().collect();
                Box::new(move || {
                    energyml_analysis::triangulate_xy(&flat).expect("triangulated");
                })
            }),
        },
        Case {
            name: "triangulate constrained",
            counts: "points, 10 traces × 50 segments",
            sizes: vec![10_000, 100_000],
            run: Box::new(|n| {
                plan(
                    "triangulate",
                    Pieces::empty()
                        .with("shape", shape(cloud(n, 9)))
                        .with("faults", shape(traces(10, 50, 10))),
                    Choices::new(),
                )
            }),
        },
        Case {
            name: "surface-map dip",
            counts: "surface nodes",
            sizes: vec![10_000, 40_000, 160_000, 640_000],
            run: Box::new(|n| {
                let geometry = surface(n, relief);
                let nodes = nodes_of(&geometry);
                plan(
                    "surface-map",
                    Pieces::empty()
                        .with("shape", shape(geometry))
                        .with("object", known("Horizon", nodes)),
                    Choices::new().set("what", "dip"),
                )
            }),
        },
        Case {
            name: "surface-map mean-curvature",
            counts: "surface nodes",
            sizes: vec![10_000, 40_000, 160_000, 640_000],
            run: Box::new(|n| {
                let geometry = surface(n, relief);
                let nodes = nodes_of(&geometry);
                plan(
                    "surface-map",
                    Pieces::empty()
                        .with("shape", shape(geometry))
                        .with("object", known("Horizon", nodes)),
                    Choices::new().set("what", "mean-curvature"),
                )
            }),
        },
        Case {
            name: "thickness isochore",
            counts: "nodes of each surface",
            sizes: vec![10_000, 40_000, 160_000, 640_000],
            run: Box::new(|n| {
                let top = surface(n, relief);
                let nodes = nodes_of(&top);
                plan(
                    "thickness",
                    Pieces::empty()
                        .with("top", shape(top))
                        .with("top (object)", known("Toit", nodes))
                        .with("base", shape(surface(n, |x, y| relief(x, y) - 50.0))),
                    Choices::new().set("what", "isochore"),
                )
            }),
        },
        Case {
            name: "isocontours",
            counts: "surface nodes, 10 levels",
            sizes: vec![10_000, 40_000, 160_000, 640_000],
            run: Box::new(|n| {
                plan(
                    "isocontours",
                    Pieces::empty().with("shape", shape(surface(n, relief))),
                    Choices::new(),
                )
            }),
        },
        Case {
            name: "intersect",
            counts: "nodes of the fault and of each of 2 horizons",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                plan(
                    "intersect",
                    Pieces::empty()
                        .with("fault", shape(fault_sheet(n)))
                        .with("surfaces", shape(surface(n, relief)))
                        .with("surfaces 2", shape(surface(n, |x, y| relief(x, y) - 300.0))),
                    Choices::new(),
                )
            }),
        },
        Case {
            name: "cross-section",
            counts: "nodes of each of 3 surfaces",
            sizes: vec![10_000, 40_000, 160_000],
            run: Box::new(|n| {
                let trace = patch(
                    vec![
                        [100.0, 200.0, 0.0],
                        [5_000.0, 5_200.0, 0.0],
                        [9_900.0, 9_700.0, 0.0],
                    ],
                    Topology::Polylines(vec![3]),
                );
                plan(
                    "cross-section",
                    Pieces::empty()
                        .with("trace", shape(trace))
                        .with("surfaces", shape(surface(n, relief)))
                        .with("surfaces 2", shape(surface(n, |x, y| relief(x, y) - 200.0)))
                        .with("surfaces 3", shape(surface(n, |x, y| relief(x, y) - 400.0))),
                    Choices::new(),
                )
            }),
        },
    ]
}

/// The exponent `k` of `t ∝ nᵏ`, by least squares on the logarithms.
fn exponent(points: &[(usize, f64)]) -> f64 {
    let logs: Vec<(f64, f64)> = points.iter().map(|&(n, t)| ((n as f64).ln(), t.ln())).collect();
    let count = logs.len() as f64;
    let (mx, my) = (
        logs.iter().map(|p| p.0).sum::<f64>() / count,
        logs.iter().map(|p| p.1).sum::<f64>() / count,
    );
    let num: f64 = logs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let den: f64 = logs.iter().map(|p| (p.0 - mx).powi(2)).sum();
    num / den
}

fn readable(seconds: f64) -> String {
    if seconds >= 1.0 {
        format!("{seconds:.2} s")
    } else if seconds >= 1e-3 {
        format!("{:.1} ms", seconds * 1e3)
    } else {
        format!("{:.0} µs", seconds * 1e6)
    }
}

fn main() {
    // `cargo bench` passes `--bench`; nothing else is read from the line.
    let only = std::env::var("SPEED_ONLY").ok();
    println!("| operation | n | median | n per second | exponent |");
    println!("|---|---:|---:|---:|---:|");
    for case in cases() {
        if only.as_deref().is_some_and(|word| !case.name.contains(word)) {
            continue;
        }
        let mut measured = Vec::new();
        for &size in &case.sizes {
            let mut run = (case.run)(size);
            let seconds = median(&mut run).as_secs_f64();
            measured.push((size, seconds));
        }
        let slope = exponent(&measured);
        for (at, &(size, seconds)) in measured.iter().enumerate() {
            let label = if at == 0 {
                format!("{} ({})", case.name, case.counts)
            } else {
                String::new()
            };
            let slope = if at == 0 {
                format!("{slope:.2}")
            } else {
                String::new()
            };
            println!(
                "| {label} | {size} | {} | {:.0} | {slope} |",
                readable(seconds),
                size as f64 / seconds
            );
        }
    }
}
