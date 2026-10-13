//! `extend-boreholes` — continue a wellbore frame below its logged base, down
//! to at most a stated depth, and mark the added part as synthetic.
//!
//! This is the borehole-extension step of the reference model's build plan:
//! extend a frame down to at most a stated depth, re-cut it, and mark it.
//! Ten BSS boreholes log only their upper few tens of metres; a
//! frame stops where the log stops, and `temperature-forages` and
//! `pression-forages` want one that reaches further down so that a law can be
//! posed on it — the steps of the reference recipe that take them as input
//! (`temperature-forages` and `pression-forages`) wait on this operation.
//!
//! # Delegated, not duplicated
//!
//! An operation that writes the same object as another delegates to it
//! (ADR-0017): the new trajectory and frame go
//! through [`super::well::write`], the one writer of the two — the measured
//! depths, the vertical detection, the parametric line's kind are all its.
//! What this module adds is the extension itself: where it stops, how it is
//! cut, and what says it is not measured.
//!
//! # A second object, never the first rewritten
//!
//! Enriching does not overwrite: identity comes from the content, and
//! recognising a borehole takes the title **and** the head, so "enriching" has a price
//! (see the identity rules): a property already laid on the old frame is
//! **positional** — insert a node in the middle and every value after it
//! describes the wrong depth. So the old trajectory and frame are left
//! exactly as they are, and the extension is a **second** pair, on the same
//! interpretation and the same datum — the MD zero has not moved, only the
//! hole has grown. There is no `enrich` setting here to ask for the other
//! thing: `Wellbore::keep` is always `None`.
//!
//! Every measured depth the old frame stated is carried **bit for bit**: the
//! new frame's control points are the old ones, unchanged, plus the ones this
//! module adds below — never a value rebuilt from the line, which is known
//! not to round-trip (a given MD is not recomputed).
//!
//! # Where it stops, and how it is cut
//!
//! `maxDepth` (200 m by default, capping the extension at 200 m of depth)
//! is the deepest measured depth the new frame may
//! reach, not a length added: ten BSS holes stop at very different depths, and
//! a flat "add 150 m" would send some of them well past what is wanted.
//! `step` cuts the added part at regular measured depths, its **last node
//! always exactly at `maxDepth`**, whatever the remainder — the same shape
//! `well::frame_mds` gives a control point between a stated spacing, written
//! out by hand here since that helper is private to its module.
//!
//! A frame already at or past `maxDepth` has nothing to extend, and is refused
//! by name rather than silently left as it is: asking to extend it is very
//! likely the wrong frame, or a `maxDepth` too shallow for what is already
//! measured — either way a word beats a quiet no-op.
//!
//! # The direction continued, and why it is honest only for a straight hole
//!
//! The added nodes continue the **last** direction the frame's own nodes
//! describe — the unit vector between its last two distinct positions, scaled
//! by each new node's distance in measured depth. That is exact for a straight
//! segment and nothing more: it is not arithmetic, it is the same kind of
//! assumption `energyml-geometry/src/well.rs` already makes for a trajectory
//! with no shape of its own — *taken because it is the only reading the data
//! permits*. A BSS hole is vertical, so the assumption costs nothing here; a
//! frame with fewer than two distinct nodes falls back to the frame's own
//! vertical direction (`Known::vertical_up`), and refuses by name when neither
//! is available.
//!
//! # Marked, never silently grown
//!
//! The rule on invented objects (ADR-0019): an `ExtensionNameValue` `synthetic =
//! true` and a sentence in `Citation/Description` on any invented object.
//! Here only **part** of the new trajectory and frame is invented, so the mark
//! says where: `synthetic` (`true`) beside `syntheticFromMd`, the measured
//! depth above which every node is the file's own and below which every one is
//! laid by this operation, at `step` from a stated `source` — required, and
//! refused blank, the same rule a constant is given (no constant without a source): a
//! plausible value is declared synthetic.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, Params, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Dropped, Identity, Need, NewObject, Note, Operation, Plan,
    Refusal, Slot, When,
};
use crate::pieces::Known;

use super::support;
use super::well::{self, TrajectoryShape, Wellbore};

/// The operation.
pub struct ExtendBoreholes;

/// What it is told.
#[derive(Debug, Params)]
pub struct ExtendParams {
    /// how deep the frame may reach, in measured depth from its datum — a frame already at or past it has nothing to extend
    #[param(default = 200.0, optional, min = 0.0, uom = "m")]
    pub max_depth: f64,
    /// a node every so many metres of measured depth in the part added — the last is always exactly at `maxDepth`, whatever the remainder
    #[param(default = 10.0, optional, min = 0.0, uom = "m")]
    pub step: f64,
    /// where the extension comes from — a document, or why the value is plausible (reference-model rule: no constant without a source): the added part is never measured, so this can never cite a log
    pub source: String,
    /// the external file the new arrays go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What it takes.
#[derive(Debug, Inputs)]
pub struct ExtendInputs {
    /// the wellbore frames to extend — left empty, every frame of the package
    #[input(least = 1, types = "resqml22.WellboreFrameRepresentation, resqml20.obj_WellboreFrameRepresentation", every)]
    pub frames: Vec<Shape>,
}

/// The unit direction to continue in — the last two distinct positions the
/// frame's own nodes describe, or its stated vertical when fewer than two
/// differ.
fn last_direction(nodes: &[[f64; 3]], vertical_up: Option<bool>) -> Result<[f64; 3], &'static str> {
    for pair in nodes.windows(2).rev() {
        let d = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1], pair[1][2] - pair[0][2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len > 1e-9 {
            return Ok([d[0] / len, d[1] / len, d[2] / len]);
        }
    }
    match vertical_up {
        Some(true) => Ok([0.0, 0.0, -1.0]),
        Some(false) => Ok([0.0, 0.0, 1.0]),
        None => {
            Err("has no two nodes that differ, and its frame states no vertical direction to fall back on: nothing says which way to continue")
        }
    }
}

/// The measured depths to add: every `step` from the one the frame already
/// reaches, the last always exactly at `maxDepth` — the shape
/// `well::frame_mds` gives a spacing that does not divide the interval evenly,
/// applied here by hand since that helper is private to its module.
fn added_mds(last: f64, step: f64, max_depth: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut md = last + step;
    while md < max_depth - step * 1e-9 {
        out.push(md);
        md += step;
    }
    out.push(max_depth);
    out
}

/// The two `ExtensionNameValue`s a partly-invented object carries — see the
/// module doc, and the rule on invented objects (ADR-0019).
fn synthetic_marks(from_md: f64, uom: &str, source: &str) -> Value {
    json!([
        {
            "$type": "eml23.ExtensionNameValue",
            "Name": "synthetic",
            "Value": { "$type": "eml23.StringMeasure", "_": "true" },
            "Description": format!(
                "below {from_md} {uom} of measured depth the nodes are laid by `extend-boreholes`, not measured — source: {source}"
            ),
        },
        {
            "$type": "eml23.ExtensionNameValue",
            "Name": "syntheticFromMd",
            "Value": { "$type": "eml23.StringMeasure", "_": format!("{from_md}"), "uom": uom },
            "MeasureClass": "length",
            "Description": "the measured depth above which every node is the file's own, and below which every one is laid",
        },
    ])
}

/// What one frame's extension produced.
struct Extended {
    objects: Vec<NewObject>,
    datasets: Vec<Dataset>,
    added: usize,
}

/// Extend one frame, or say why it could not be.
fn extend_one(
    one: &Shape,
    params: &ExtendParams,
    identity: &Identity,
    when: &When,
    rank: &mut usize,
) -> Result<Extended, String> {
    let known: &Known = one.known.as_ref().ok_or_else(|| {
        format!("`{}` states nothing of itself — no wellhead, no measured depths, no datum, no interpretation", one.name)
    })?;
    let frame_ref = known.reference.as_ref().ok_or_else(|| format!("`{}` is not itself an object of the package", one.name))?;
    let title = frame_ref.title.clone();
    let Some(mds) = known.node_mds.as_deref().filter(|m| !m.is_empty()) else {
        return Err(format!("`{title}` states no measured depths for its nodes"));
    };
    let Some(datum) = known.datum.as_ref() else {
        return Err(format!("`{title}` names no datum: nothing states where its measured depths are counted from"));
    };
    let Some(interpretation) = known.interpretation.as_ref() else {
        return Err(format!("`{title}` names no interpretation: a second trajectory has nothing to hang off"));
    };
    let Some(crs) = known.crs.as_ref() else {
        return Err(format!("`{title}` names no local CRS: nothing states the frame the new nodes would be written in"));
    };
    let Some(uom) = known.vertical_uom.clone() else {
        return Err(format!("`{title}` states no unit for its measured depths"));
    };
    let Some(wellhead) = known.wellhead else {
        return Err(format!("`{title}` states no wellhead: nothing says where it is collared"));
    };

    let nodes: Vec<[f64; 3]> = one.geometry.patches.iter().flat_map(|patch| patch.points.iter().copied()).collect();
    if nodes.len() != mds.len() {
        return Err(format!(
            "`{title}` has {} node(s) for {} measured depth(s): they should be the same",
            nodes.len(),
            mds.len()
        ));
    }

    let last_md = *mds.last().expect("checked non-empty above");
    if last_md >= params.max_depth {
        return Err(format!("`{title}` already reaches {last_md} {uom}, at or beyond the {} {uom} limit", params.max_depth));
    }

    let direction = last_direction(&nodes, known.vertical_up).map_err(|why| format!("`{title}` {why}"))?;
    let new_mds = added_mds(last_md, params.step, params.max_depth);
    let last_point = nodes[nodes.len() - 1];

    let mut line = nodes;
    let mut all_mds = mds.to_vec();
    for &md in &new_mds {
        let d = md - last_md;
        line.push([
            last_point[0] + direction[0] * d,
            last_point[1] + direction[1] * d,
            last_point[2] + direction[2] * d,
        ]);
        all_mds.push(md);
    }

    // **The title does not change** (an enrichment keeps the title and writes
    // no version field) —
    // `well::write` builds `"{base} — trajectory"` and `"{base} — frame"`, so
    // `base` is what the old frame's own title was built from.
    let base_title = title.strip_suffix(" — frame").unwrap_or(&title).to_string();
    // **The identity comes from the content**: the frame continued, the
    // measured depths added — a correction to either is a second extension,
    // never the first rewritten in place. **The `source` text is not in it**:
    // it says where the invented values come from, not what the object is, so
    // rewording a citation must not rename the object.
    let seed = format!(
        "extend-borehole:{}:{}",
        frame_ref.uuid.to_ascii_lowercase(),
        new_mds.iter().map(|v| format!("{:?}", super::support::seed_mm(*v))).collect::<Vec<_>>().join(",")
    );
    let cite = |name: &str| support::citation(when, name);
    // Bound once, rather than as inline temporaries: three references into
    // the same statement is exactly the case worth naming instead of trusting.
    let datum_dor = datum.dor();
    let interpretation_dor = interpretation.dor();
    let crs_dor = crs.dor();
    let written = well::write(
        &Wellbore {
            title: &base_title,
            seed: &seed,
            line: &line,
            mds: Some(&all_mds),
            ground: wellhead[2],
            uom: &uom,
            shape: TrajectoryShape::Linear,
            tangents: None,
            spacing: None,
            datum: &datum_dor,
            interpretation: &interpretation_dor,
            file: &params.data_file,
            vertical_up: known.vertical_up,
            crs: &crs_dor,
            citation: &cite,
            // **Never** — the old trajectory and frame are left exactly as
            // they are; see the module doc.
            keep: None,
        },
        identity,
        rank,
    )
    .map_err(|why| why.to_string())?;

    let marks = synthetic_marks(last_md, &uom, &params.source);
    let description = format!(
        "Continues `{title}` below {last_md} {uom} of measured depth, laid at {} {uom} steps to {} {uom} — not measured. Source: {}",
        params.step, params.max_depth, params.source
    );
    let mut objects = written.objects;
    for object in &mut objects {
        if object.role == "trajectory" || object.role == "frame" {
            object.json["Citation"]["Description"] = json!(description);
            object.json["ExtensionNameValue"] = marks.clone();
        }
    }

    Ok(Extended { objects, datasets: written.datasets, added: new_mds.len() })
}

impl Operation for ExtendBoreholes {
    type Params = ExtendParams;
    type Inputs = ExtendInputs;

    fn about(&self) -> About {
        About {
            name: "extend-boreholes",
            describe: "continue a wellbore frame below its logged base, at most to a stated depth, on a second trajectory and frame marked as laid rather than measured",
            category: Category::Synthetic,
            makes: &["resqml22.WellboreTrajectoryRepresentation", "resqml22.WellboreFrameRepresentation"],
            recipe: "extend-boreholes",
            represents: None,
        }
    }

    fn plan(&self, inputs: ExtendInputs, params: ExtendParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        if params.step <= 0.0 {
            return Err(Refusal::on(
                "step",
                format!("is {}, and a node every zero or negative metres never reaches `maxDepth`", params.step),
            ));
        }
        if params.max_depth <= 0.0 {
            return Err(Refusal::on(
                "maxDepth",
                format!("is {}, and a frame cannot be extended to at or above the ground", params.max_depth),
            ));
        }
        if params.source.trim().is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: the added part is never measured, so a document or a stated plausibility is required (reference-model rule: no constant without a source)",
            ));
        }
        // A defensive ceiling, the same reasoning `interpolate` already gives
        // an absurd grid: a step far finer than `maxDepth` would ask this
        // build to invent an unreasonable number of nodes for no well anybody
        // measured.
        if params.max_depth / params.step > 1_000_000.0 {
            return Err(Refusal::on(
                "step",
                format!(
                    "would add up to {} node(s) to reach {} {}: too fine to be useful on a borehole frame",
                    (params.max_depth / params.step).ceil(),
                    params.max_depth,
                    "m"
                ),
            ));
        }
        if inputs.frames.is_empty() {
            return Err(Refusal::on(
                "frames",
                "it takes wellbore frames to extend; the input holds none, and the package gives none either",
            ));
        }

        let identity = cx.identity;
        let when = cx.when;
        let mut rank = 0usize;
        let mut objects = Vec::new();
        let mut datasets = Vec::new();
        let mut aside: BTreeMap<String, usize> = BTreeMap::new();
        let mut extended = 0usize;
        let mut nodes_added = 0usize;

        for one in &inputs.frames {
            match extend_one(one, &params, identity, when, &mut rank) {
                Ok(made) => {
                    extended += 1;
                    nodes_added += made.added;
                    objects.extend(made.objects);
                    datasets.extend(made.datasets);
                }
                Err(why) => *aside.entry(why).or_default() += 1,
            }
        }

        // **Nothing to write is refused, never a quiet empty plan.** With one
        // frame given, this is the refusal a caller sees by name — the same
        // sentence the aggregate below only joins.
        if extended == 0 {
            let why = aside.iter().map(|(why, count)| format!("{count} × {why}")).collect::<Vec<_>>().join("; ");
            return Err(Refusal::on("frames", format!("no frame could be extended: {why}")));
        }

        let says = vec![
            Note {
                key: "extended",
                detail: format!("{extended} frame(s) extended, {nodes_added} node(s) added, to at most {} m", params.max_depth),
            },
            Note {
                key: "synthetic",
                detail: "the part added is laid, not measured: it carries `synthetic = true` and `syntheticFromMd` on the new trajectory and frame".to_string(),
            },
        ];

        let seed = cx.seed("extend-boreholes");
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets,
            // **No shape**, for the reason `well-from-polyline` already gives:
            // what is written is a parametric line, whose control points are
            // its trajectory only when it is linear.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "extend-boreholes",
                title: format!("extend-boreholes → {extended} frame(s) to {} m", params.max_depth),
                seed,
                parameters: vec![
                    Parameter::Double { title: "maxDepth".to_string(), value: params.max_depth, uom: Some("m".to_string()) },
                    Parameter::Double { title: "step".to_string(), value: params.step, uom: Some("m".to_string()) },
                    Parameter::Text { title: "source".to_string(), value: params.source.clone() },
                    Parameter::Double { title: "frameCount".to_string(), value: extended as f64, uom: None },
                    Parameter::Double { title: "nodeCount".to_string(), value: nodes_added as f64, uom: None },
                ],
            },
            says,
            dropped: aside.into_iter().map(|(why, count)| Dropped { count, why }).collect(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(name, Need::Number { uom: None, least: None, most: None }, about)
        };
        vec![
            count("frameCount", "frames extended"),
            count("nodeCount", "nodes added across every frame extended"),
        ]
    }
}
