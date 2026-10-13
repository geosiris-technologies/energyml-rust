//! `horizon-from-law` — a horizon where no borehole measures it, laid by a
//! **sourced law** below or above a reference surface and cut by the erosion
//! surfaces above it; written as a 2D grid on the reference's own lattice.
//!
//! This is the law-horizon step of the reference model's build plan: horizons
//! where no borehole measures them, with sourced thicknesses. Two horizons of
//! the model are measured by no borehole: the top of the Lutetian, synthetic,
//! under the south-east relief only and elsewhere truncated by an older
//! horizon or by the embankments; and the top of the Santonian, synthetic, under
//! every measurement (the deepest borehole stops 150.7 m into the Campanian).
//! What they need is what this
//! declares:
//!
//! - **a thickness from a reference** — one horizon under another, one over the
//!   base of the Lutetian — the same everywhere (`law: parallel`) or changing along a
//!   direction (`law: linear`), a union of cases like `interpolate`'s method;
//! - **a truncation** — an older horizon stops a younger one (a rule of the
//!   reference model) — by
//!   the erosion surfaces given as `cuts`.
//!
//! A plane given by a point, a dip and an azimuth is **not** offered: no
//! horizon of the reference model is one. Its planar surfaces are faults,
//! triangulated sets written by the faults step.
//!
//! # The computation is `energyml_analysis::horizon_law`
//!
//! The house rule: a computation lives in `energyml-analysis`, never in an
//! operation (ADR-0017). That module says what a node answers — laid, a hole of the
//! reference, eroded, under a folded cut, beyond the cuts — and this one counts
//! each in the activity and says each in the plan. The cuts are read by
//! `energyml_analysis::heights` through `support::Faces`, the one reading of
//! *the height of a surface under this (x, y)* that `thickness` and `project`
//! share: a grid's cells bilinearly, triangles linearly.
//!
//! # The writing is `grid`'s
//!
//! An operation that writes the same object as another delegates to it: the new
//! horizon goes to [`super::grid::write`] as a [`Lattice`] — the reference's
//! origin, spacing and counts, and the laid heights in the house order, `i`
//! along x fastest. So the reference **is a 2D grid**, refused by name at the
//! offer otherwise (lay the law on a triangulated surface and write it as a
//! grid would be choosing a lattice nobody stated; `interpolate` makes a grid
//! of a surface first). Its lattice is read off its nodes and must be the
//! house one — along +x then +y, regular — or the plan is refused naming the
//! first node that is not: `grid` writes no rotated or reversed lattice.
//!
//! # One frame, the reference's
//!
//! The law is laid on the reference's heights, so the new horizon **is** in
//! the reference's frame: `crs` is suggested from it, and a `crs` naming
//! another system — or a frame created here with `sourceEpsg` — is refused,
//! the heights would otherwise be read against another origin. A cut in
//! another frame, or counting in another unit, is refused the way `thickness`
//! refuses two frames: a height compared across two origins is a comparison of
//! origins. A reference in **time** is refused: a thickness is a length.
//!
//! # A thickness is sourced, and the horizon says it was laid
//!
//! No constant without a source: a plausible value is declared
//! synthetic, and a value presented as real cites its document
//! (reference-model rule, ADR-0019). `source` is **required**, and a blank
//! one is refused by name: a document with its page, or a plausible value said
//! to be one.
//!
//! And the rule on invented objects — an `ExtensionNameValue` `synthetic = true` and a sentence in
//! `Citation/Description` on every invented object — is written on the grid,
//! whatever the source of its thickness: a surface laid by a law is not a
//! measurement. `ExtensionNameValue` is the eml 2.3 `AbstractObject` field for
//! it (`Abstract.xsd`, `ExtensionNameValue`), and the description stays within
//! the 2000 characters `Citation/Description` allows.
//!
//! # What is not suggested
//!
//! `interpretation` and `title`, which the shared block would read off the
//! reference: the horizon laid is **another** horizon, and one filed under the reference's
//! interpretation — or titled after it — is the error a suggestion would make
//! look deliberate.
//!
//! # Identity
//!
//! Named like `interpolate`'s grids — by what the horizon is, its reference and
//! its title — and not by its law: restating a thickness rewrites the same
//! horizon, which is what correcting a recipe means (correcting means editing
//! the recipe and replaying it).
//!
//! # References
//!
//! - The horizon law is a synthetic law stated in `energyml-analysis`, which cites no published source for it.
//!   TODO(reference): verify — no publication is cited; if the law follows one, add it (authors, year, title, venue, DOI) once checked.

use serde_json::json;

use energyml_analysis::horizon_law::{self as analysis, Law};
use energyml_analysis::surface::Up;
use energyml_domain::provenance::Parameter;
use energyml_domain::units::Dimension;
use energyml_geometry::geometry::{Geometry, Topology};

use crate::declare::{Choice, Inputs, Params, Shape, Suggestions, known_of};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Lattice, Pieces};

use super::support;
use super::thickness::Vertical;
use super::written::Written;

/// The operation.
pub struct HorizonFromLaw;

/// What `horizon-from-law` is told.
#[derive(Debug, Params)]
pub struct HorizonLawParams {
    /// the thickness between the reference and the new horizon, measured vertically, in the unit of the reference's heights — for a linear law, the thickness at its anchor
    ///
    /// Vertical — an isochore — because a node of a grid carries one height
    /// per (x, y). Where the reference dips, the thickness across the beds is
    /// this times the cosine of the dip, and the plan says so.
    #[param(min = 0.0)]
    pub thickness: f64,
    /// The law the thickness follows.
    #[param(flatten)]
    pub law: LawParams,
    /// which side of the reference the new horizon lies on — below it (older, deeper) or above it (younger, shallower)
    ///
    /// No default: which side is the geology, and a default would state it
    /// for whoever did not.
    pub side: Side,
    /// what the third coordinate of the reference measures — an elevation (larger is higher) or a depth (larger is lower)
    ///
    /// Suggested from the reference's frame, never guessed: *below* is down,
    /// and down is −z in elevation and +z in depth.
    pub axis: Vertical,
    /// where the thickness comes from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// The frame, what is represented, the naming — for the grid writer.
    #[param(flatten)]
    pub written: Written,
}

/// the law the thickness follows — the same everywhere, or changing linearly along a direction
#[derive(Debug, Params)]
#[params(tag = "law")]
pub enum LawParams {
    /// The same thickness everywhere: a surface parallel to the reference.
    #[case(default)]
    Parallel,
    /// A thickness changing linearly along a direction.
    Linear(LinearParams),
}

/// What a linear law adds to its thickness.
#[derive(Debug, Params)]
pub struct LinearParams {
    /// linear: how much the thickness changes per unit of horizontal distance along the azimuth — positive thickens that way, negative thins
    pub gradient: f64,
    /// linear: the direction the gradient runs along, clockwise from grid north
    #[param(min = 0.0, max = 360.0, uom = "dega")]
    pub azimuth: f64,
    /// linear: the x of the point where the thickness is `thickness`, in the reference's frame
    pub anchor_x: f64,
    /// linear: the y of that point, in the reference's frame
    pub anchor_y: f64,
}

/// Which side of the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Side {
    /// Below it: older, deeper.
    Below,
    /// Above it: younger, shallower.
    Above,
}

/// What `horizon-from-law` takes.
#[derive(Debug, Inputs)]
pub struct HorizonLawInputs {
    /// the surface the law is laid from — a 2D grid, whose lattice the new horizon takes
    #[input(topology = "grid")]
    pub reference: Shape,
    /// erosion surfaces the new horizon may not rise above — where one lies below it, the horizon is eroded and left a hole
    ///
    /// `triangles` ranks the form's list; the cells of a grid are read too,
    /// bilinearly — the reading `thickness` gives its base.
    #[input(topology = "triangles")]
    pub cuts: Vec<Shape>,
}

/// A count, as the activity records it.
fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    }
}

/// The title the object behind a shape carries, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The counts of a grid shape — `[ni, nj]` — or why it is none a law is laid
/// on.
fn grid_counts(geometry: &Geometry) -> Result<[usize; 2], String> {
    let [patch] = geometry.patches.as_slice() else {
        return Err(format!(
            "holds {} patch(es): the new horizon is written on the lattice of one 2D grid",
            geometry.patches.len()
        ));
    };
    let Topology::Grid { fastest, slowest } = &patch.topology else {
        return Err(
            "is no 2D grid: the new horizon is written on the reference's lattice, so the reference is a Grid2dRepresentation — make one of a surface or of points with `interpolate` first"
                .to_string(),
        );
    };
    let (ni, nj) = (*fastest, *slowest);
    if ni < 2 || nj < 2 {
        return Err(format!(
            "is a grid of {ni} × {nj} nodes: a lattice has at least 2 along each axis"
        ));
    }
    if patch.points.len() != ni * nj {
        return Err(format!(
            "declares {ni} × {nj} nodes and holds {}",
            patch.points.len()
        ));
    }
    Ok([ni, nj])
}

/// A lattice as [`lattice_of`] reads it: origin, spacing, counts `[ni, nj]`.
type LatticeRead = ([f64; 2], [f64; 2], [usize; 2]);

/// The reference's lattice — origin, spacing, counts — read off its nodes, or
/// the first node that is not where the house lattice puts it.
///
/// The house order is `grid`'s (a grid is never reprojected, and its order is the house order):
/// `i` along +x varies fastest, `j` along +y. A grid
/// read from a file comes in the order its `FastestAxisCount` says
/// (`energyml_geometry`, `grid_2d`), so a rotated lattice, or one running
/// toward −x or −y, is refused rather than re-indexed here: `grid` would write
/// it on axes it does not have.
fn lattice_of(geometry: &Geometry) -> Result<LatticeRead, String> {
    let [ni, nj] = grid_counts(geometry)?;
    let points = &geometry.patches[0].points;
    let at = |i: usize, j: usize| points[j * ni + i];
    let origin = at(0, 0);
    let (dx, dy) = (at(1, 0)[0] - origin[0], at(0, 1)[1] - origin[1]);
    if !(dx.is_finite() && dx > 0.0 && dy.is_finite() && dy > 0.0) {
        return Err(format!(
            "runs along ({}, {}) then ({}, {}): `grid` writes a lattice along +x then +y, and a rotated or reversed one is not re-indexed here",
            at(1, 0)[0] - origin[0],
            at(1, 0)[1] - origin[1],
            at(0, 1)[0] - origin[0],
            at(0, 1)[1] - origin[1]
        ));
    }
    // A millionth of a cell: what the reader's running sum of spacings can
    // drift by, and nothing a producer means.
    let tolerance = 1e-6 * dx.min(dy);
    for j in 0..nj {
        for i in 0..ni {
            let node = at(i, j);
            let (x, y) = (origin[0] + i as f64 * dx, origin[1] + j as f64 * dy);
            if !((node[0] - x).abs() <= tolerance && (node[1] - y).abs() <= tolerance) {
                return Err(format!(
                    "node ({i}, {j}) is at ({}, {}) where a regular lattice along +x then +y puts it at ({x}, {y}): a rotated or irregular grid is not what `grid` writes",
                    node[0], node[1]
                ));
            }
        }
    }
    Ok(([origin[0], origin[1]], [dx, dy], [ni, nj]))
}

/// The law, in the words a description and a note use.
fn law_words(law: &Law, unit: &str) -> String {
    match *law {
        Law::Parallel { thickness } => format!("{thickness} {unit}, the same everywhere"),
        Law::Linear {
            thickness,
            anchor,
            azimuth,
            gradient,
        } => format!(
            "{thickness} {unit} at ({}, {}), changing by {gradient} {unit} per unit of horizontal distance toward {azimuth}° from grid north",
            anchor[0], anchor[1]
        ),
    }
}

impl Operation for HorizonFromLaw {
    type Params = HorizonLawParams;
    type Inputs = HorizonLawInputs;

    fn about(&self) -> About {
        About {
            name: "horizon-from-law",
            describe: "a horizon where no borehole measures it — laid at a sourced thickness below or above a reference grid, and cut by the erosion surfaces above it — written as a 2D grid on the reference's lattice",
            category: Category::Synthetic,
            makes: &["resqml22.Grid2dRepresentation"],
            recipe: "horizon-from-law",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
        // **Not the reference's interpretation, nor its title**: the horizon
        // laid is another one — see the module.
        for name in ["interpretation", "title"] {
            if let Some(slot) = slots.slot_mut(name) {
                slot.suggested = None;
                slot.from = None;
            }
        }
        let Some(known) = known_of(bag, "reference") else {
            return;
        };
        // The reference's frame, whatever the cuts name: the law is laid in it.
        if let Some(crs) = &known.crs {
            slots.inferred("crs", crs.dor(), crs.clone());
        }
        // From the frame the reader read, never from the heights.
        if let Some(up) = known.vertical_up {
            slots.set("axis", json!(if up { "elevation" } else { "depth" }));
        }
    }

    fn check(&self, inputs: &HorizonLawInputs) -> Result<(), Refusal> {
        grid_counts(&inputs.reference.geometry).map_err(|why| Refusal::on("reference", why))?;
        for cut in &inputs.cuts {
            if !support::Faces::of(&cut.geometry).any() {
                return Err(Refusal::on(
                    "cuts",
                    format!(
                        "`{}` has no face: an erosion surface is read under each node of the horizon, and points or lines have none",
                        cut.name
                    ),
                ));
            }
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: HorizonLawInputs,
        params: HorizonLawParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let HorizonLawInputs { reference, cuts } = inputs;
        let (origin, spacing, counts) =
            lattice_of(&reference.geometry).map_err(|why| Refusal::on("reference", why))?;

        // -- the source, before anything is laid -------------------------------
        let source = params.source.trim().to_string();
        if source.is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: a thickness is sourced — a document with its page or paragraph, or a plausible value said to be one (reference-model rule: no constant without a source)",
            ));
        }

        // -- one frame, the reference's ------------------------------------------
        let known = reference.known.as_ref();
        let frame = known.and_then(|k| k.crs.as_ref());
        let unit = known.and_then(|k| k.vertical_uom.clone());
        if let Some(unit) = &unit
            && Dimension::of(unit) == Some(Dimension::Time)
        {
            return Err(Refusal::on(
                "reference",
                format!(
                    "counts its heights in `{unit}`, a time: a thickness is a length — convert the reference to depth first"
                ),
            ));
        }
        for cut in &cuts {
            let theirs = cut.known.as_ref();
            if let (Some(a), Some(b)) = (frame, theirs.and_then(|k| k.crs.as_ref()))
                && !a.uuid.eq_ignore_ascii_case(&b.uuid)
            {
                return Err(Refusal::new(
                    "frames",
                    format!(
                        "`reference` is in `{}` and the cut `{}` in `{}`: a height compared across two frames is a comparison of two origins — bring them into one frame first",
                        a.title,
                        title_of(cut),
                        b.title
                    ),
                ));
            }
            if let (Some(a), Some(b)) = (&unit, theirs.and_then(|k| k.vertical_uom.as_ref()))
                && a != b
            {
                return Err(Refusal::new(
                    "units",
                    format!(
                        "`reference` counts its heights in `{a}` and the cut `{}` in `{b}` — one unit, or no comparison",
                        title_of(cut)
                    ),
                ));
            }
        }
        if let Some(frame) = frame {
            match (&params.written.frame.crs, params.written.frame.source_epsg) {
                (Some(named), _) => {
                    let uuid = named
                        .0
                        .get("Uuid")
                        .and_then(|v| v.as_str())
                        .or_else(|| named.0.as_str())
                        .unwrap_or_default();
                    if !uuid.eq_ignore_ascii_case(&frame.uuid) {
                        return Err(Refusal::on(
                            "crs",
                            format!(
                                "names another frame than the reference's `{}`: the law is laid on the reference's heights, so the new horizon is in its frame — name it, or leave `crs` out to take it",
                                frame.title
                            ),
                        ));
                    }
                }
                (None, Some(_)) => {
                    return Err(Refusal::on(
                        "sourceEpsg",
                        format!(
                            "would create a frame, and the reference's heights are in `{}`: name that one as `crs`, or leave both out to take it",
                            frame.title
                        ),
                    ));
                }
                (None, None) => {}
            }
        }

        // -- the law, laid ---------------------------------------------------------
        let law = match &params.law {
            LawParams::Parallel => Law::Parallel {
                thickness: params.thickness,
            },
            LawParams::Linear(linear) => Law::Linear {
                thickness: params.thickness,
                anchor: [linear.anchor_x, linear.anchor_y],
                azimuth: linear.azimuth,
                gradient: linear.gradient,
            },
        };
        let side = match params.side {
            Side::Below => analysis::Side::Below,
            Side::Above => analysis::Side::Above,
        };
        let up = match params.axis {
            Vertical::Elevation => Up::PlusZ,
            Vertical::Depth => Up::MinusZ,
        };
        let faces: Vec<support::Faces> = cuts.iter().map(|cut| support::Faces::of(&cut.geometry)).collect();
        let surfaces: Vec<analysis::Surface<'_>> = faces.iter().map(|one| one.surface()).collect();
        cx.stop_if_cancelled()?;
        let laid = analysis::lay(&reference.geometry.patches[0].points, &law, side, up, &surfaces).map_err(
            |why| match &why {
                analysis::LawError::Cut { .. } => Refusal::on("cuts", why.to_string()),
                analysis::LawError::Negative { .. } => Refusal::on("gradient", why.to_string()),
                analysis::LawError::NotFinite { .. } => Refusal::on("law", why.to_string()),
                analysis::LawError::BadThickness { .. } | analysis::LawError::ZeroThickness => {
                    Refusal::on("thickness", why.to_string())
                }
            },
        )?;
        cx.stop_if_cancelled()?;
        let analysis::Laid {
            z,
            holes,
            eroded,
            folded,
            beyond,
            set_aside,
            range,
        } = laid;
        if !z.iter().any(|one| one.is_finite()) {
            // A grid of holes would read as a horizon with nothing to show —
            // the case that looks most like a success.
            return Err(Refusal::new(
                "refused",
                format!(
                    "no node of the horizon is left: {holes} hole(s) of the reference, {eroded} eroded, {folded} under a folded cut, of {} — nothing would be drawn",
                    z.len()
                ),
            ));
        }

        // -- the writing is `grid`'s -------------------------------------------------
        let reference_title = title_of(&reference);
        let reference_key = known
            .and_then(|k| k.reference.as_ref())
            .map(|r| r.uuid.clone())
            .unwrap_or_else(|| reference.name.clone());
        let title = params.written.naming.title();
        let name = format!("horizon-from-law of {}", reference.name);
        let seed = format!("horizon-from-law:{reference_key}:{title}");
        let lattice = Lattice {
            origin,
            spacing,
            counts,
            z,
        };
        let mut plan = super::grid::write(&name, &lattice, &params.written, &seed, cx)?;

        // What the grid says of itself: laid, not measured, and from what.
        let unit_word = unit
            .clone()
            .unwrap_or_else(|| "(the unit of the reference's heights)".to_string());
        let side_word = params.side.word();
        let cut_titles: Vec<String> = cuts.iter().map(title_of).collect();
        let stated = law_words(&law, &unit_word);
        let description = format!(
            "Laid by a law, not measured: {stated}, {side_word} `{reference_title}`, measured vertically{}. Source of the thickness: {source}",
            if cut_titles.is_empty() {
                String::new()
            } else {
                format!(", cut by `{}`", cut_titles.join("`, `"))
            }
        );
        // `Citation/Description` holds 2000 characters (eml 2.3 `Citation`,
        // `DescriptionString`): refused rather than cut short, since a source
        // cut short cites nothing.
        if description.chars().count() > 2000 {
            return Err(Refusal::on(
                "source",
                format!(
                    "makes a description of {} characters, and a Citation/Description holds 2000 — cite the document shorter",
                    description.chars().count()
                ),
            ));
        }
        if let Some(object) = plan
            .objects
            .iter_mut()
            .find(|object| object.role == "representation")
        {
            object.json["Citation"]["Description"] = json!(description);
            // The reference-model rule on invented objects (ADR-0019): an
            // `ExtensionNameValue` `synthetic = true` and a sentence in `Citation/Description`
            // on any invented object.
            object.json["ExtensionNameValue"] = json!([{
                "$type": "eml23.ExtensionNameValue",
                "Name": "synthetic",
                "Value": { "$type": "eml23.StringMeasure", "_": "true" },
                "Description": "laid by `horizon-from-law` at a stated thickness from another horizon, not measured",
            }]);
        }

        plan.activity.recipe = "horizon-from-law";
        plan.activity.title = format!("horizon-from-law → {title}");
        // The thickness **with its unit**, which the registry could not know:
        // it is the reference's, read off its frame.
        plan.activity.parameters.push(Parameter::Double {
            title: "thickness".to_string(),
            value: params.thickness,
            uom: unit.clone(),
        });
        plan.activity.parameters.push(double("holeCount", holes as f64));
        plan.activity
            .parameters
            .push(double("erodedCount", eroded as f64));
        plan.activity
            .parameters
            .push(double("foldedCount", folded as f64));
        if !cuts.is_empty() {
            plan.activity
                .parameters
                .push(double("beyondCutsCount", beyond as f64));
        }

        let mut say = |key: &'static str, detail: String| plan.says.push(Note { key, detail });
        say(
            "law",
            format!("{stated}, {side_word} `{reference_title}` — source: {source}"),
        );
        say(
            "vertical-thickness",
            "the thickness is laid vertically, as an isochore: where the reference dips, the thickness across the beds is the vertical one times the cosine of the dip".to_string(),
        );
        say(
            "synthetic",
            "the horizon is laid, not measured: it carries `synthetic = true` and a description of its law and its source".to_string(),
        );
        if let (Law::Linear { .. }, Some([low, high])) = (&law, range) {
            say(
                "thickness-range",
                format!("the thickness laid runs from {low} to {high} {unit_word} over the nodes kept"),
            );
        }
        if holes > 0 {
            say(
                "reference-holes",
                format!(
                    "{holes} node(s) of `{reference_title}` have no height: the law has nothing to stand on there — holes"
                ),
            );
        }
        if eroded > 0 {
            say(
                "eroded",
                format!(
                    "{eroded} node(s) lie above a cut: eroded, written as holes — the horizon stops at the erosion surface, it does not follow it"
                ),
            );
        }
        if folded > 0 {
            say(
                "folded",
                format!(
                    "{folded} node(s) lie under a folded cut, whose sheets disagree: which one erodes is not a numerical choice — holes"
                ),
            );
        }
        if beyond > 0 {
            say(
                "beyond-cuts",
                format!(
                    "{beyond} node(s) have no cut over or under them: nothing erodes them there, and they are kept"
                ),
            );
        }
        if set_aside > 0 {
            say(
                "set-aside",
                format!(
                    "{set_aside} face(s) of the cuts set aside: a corner with no height (a hole of a cut), no area in plan, or a cell that is not convex"
                ),
            );
        }
        Ok(plan)
    }

    /// `thickness`, `law`, `side`, `axis` and `source` are inputs, recorded
    /// under the parameters the projection already declares — a title cannot
    /// be both.
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
        support::frame_reports(vec![
            count("nodeCount", "nodes written"),
            count("fastestAxisCount", "nodes along x"),
            count("slowestAxisCount", "nodes along y"),
            count(
                "holeCount",
                "nodes of the reference with no height — holes of the new horizon too",
            ),
            count("erodedCount", "nodes a cut lies below — eroded, written as holes"),
            count(
                "foldedCount",
                "nodes under a folded cut — undecided, written as holes",
            ),
            Slot::optional(
                "beyondCutsCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "nodes no cut lies over or under — nothing erodes them; with cuts only",
            ),
        ])
    }
}
