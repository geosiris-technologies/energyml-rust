//! `thickness` — the thickness between two horizons at every node of the upper
//! one, written as a continuous property on it: the isochore, or the isopach.
//!
//! The computation is
//! [`energyml_analysis::thickness`]; this is the seam, and the writing is
//! [`super::property`]'s, the way `surface-map` does it.
//!
//! **The first operation written against typed parameters and inputs**
//!: every setting is
//! a field of [`ThicknessParams`], its default written once; the two surfaces
//! are fields of [`ThicknessInputs`], bound by the framework — `plan` reads no
//! name and knows no `(object)` suffix.
//!
//! # Two roles, read by the slot's name
//!
//! `top` and `base` are two surfaces, and nothing in their geometry says which
//! is which — an interpretation can have its picks crossed, and that is one of
//! the things this measures. The roles are the names of the pieces, never an
//! altitude: sorting by height would decide exactly where the answer is
//! interesting, and a thickness computed upside down is the negative of the
//! right one, which looks like a thickness.
//!
//! # One frame, or no difference
//!
//! A difference of heights read in two frames is a difference of two origins.
//! When both inputs say their CRS and the two differ, the plan is refused and
//! names them; the same for two vertical units. When neither says, nothing can
//! be checked — the inputs of the scene are all placed in one frame, which is
//! the ordinary case of a thickness drawn from artefacts.
//!
//! # A time vertical is an isochron
//!
//! Two horizons in two-way time give a difference in **time** — the dictionary
//! calls it `vertical time thickness`, and that is what the isochore writes
//! then. The isopach is refused: its dip mixes a horizontal length with a
//! vertical time, which is not an angle.
//!
//! # References
//!
//! - Isochore (vertical thickness) and isopach (stratigraphic thickness, vertical thickness times the cosine of the top's dip): the usual geological definitions.
//!   TODO(reference): verify - no textbook or standard is cited for the isopach formula or its parallel-beds approximation; add one once checked.

use serde_json::json;

use energyml_analysis::surface::Up;
use energyml_analysis::thickness::{self as analysis, Measure};
use energyml_domain::provenance::Parameter;
use energyml_domain::units::Dimension;

use crate::declare::{
    Choice, Inputs, ObjectRef, Params, PropertyKindRef, Shape, Suggestions, Title, known_of,
};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::support;

/// The operation.
pub struct Thickness;

/// Which thickness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum What {
    /// True vertical thickness.
    Isochore,
    /// True stratigraphic thickness, by the parallel-beds formula.
    Isopach,
}

/// Which way the third coordinate points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Vertical {
    /// Larger is higher.
    Elevation,
    /// Larger is lower.
    Depth,
}

/// What `thickness` is told.
#[derive(Debug, Params)]
pub struct ThicknessParams {
    /// isochore (vertical) or isopach (perpendicular to the beds)
    #[param(default = What::Isochore)]
    pub what: What,
    /// what the third coordinate measures — an elevation (larger is higher) or a depth (larger is lower)
    pub vertical: Vertical,
    /// the representation the values are laid on — the top's
    pub representation: ObjectRef,
    /// what is measured; left out, the kind of the standard dictionary for the measure
    pub property_kind: Option<PropertyKindRef>,
    /// the unit of the values; left out, the vertical unit of the frame
    pub uom: Option<String>,
    /// the property's title; left out, the two surfaces' and the measure's
    ///
    /// Not suggested: a suggestion is settled before `plan`, so it could not
    /// follow `what` — see `surface-map`.
    pub title: Option<Title>,
    /// the external file the values go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What `thickness` takes.
#[derive(Debug, Inputs)]
pub struct ThicknessInputs {
    /// the upper surface — the values are written on its nodes
    pub top: Shape,
    /// the lower surface — triangles, or the cells of a grid
    ///
    /// `triangles` ranks the form's list; the cells of a grid are read too —
    /// bilinearly — and [`Operation::check`] is what decides.
    #[input(topology = "triangles")]
    pub base: Shape,
}

/// The title the reference of a surface gives it, or its role.
fn title_of(shape: &Shape, role: &str) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| role.to_string())
}

impl Operation for Thickness {
    type Params = ThicknessParams;
    type Inputs = ThicknessInputs;

    fn about(&self) -> About {
        About {
            name: "thickness",
            describe: "the thickness between two horizons — isochore or isopach — at every node of the top, written as a continuous property",
            category: Category::Analysis,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "thickness",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some(known) = known_of(bag, "top") else { return };
        if let Some(reference) = &known.reference {
            slots.inferred("representation", reference.dor(), reference.clone());
        }
        // From the CRS the reader read, never from the shapes: which of two
        // surfaces is higher is what this measures, not what it may assume.
        if let Some(up) = known.vertical_up {
            slots.set("vertical", json!(if up { "elevation" } else { "depth" }));
        }
    }

    fn check(&self, inputs: &ThicknessInputs) -> Result<(), Refusal> {
        if inputs
            .top
            .geometry
            .patches
            .iter()
            .all(|patch| patch.points.is_empty())
        {
            return Err(Refusal::on("top", "holds no node"));
        }
        if !support::Faces::of(&inputs.base.geometry).any() {
            return Err(Refusal::on(
                "base",
                "has no face: its height is read under each node of the top, and points or lines have none",
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: ThicknessInputs,
        params: ThicknessParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let ThicknessInputs { top, base } = inputs;
        let measure = match params.what {
            What::Isochore => Measure::Vertical,
            What::Isopach => Measure::Stratigraphic,
        };
        let up = match params.vertical {
            Vertical::Elevation => Up::PlusZ,
            Vertical::Depth => Up::MinusZ,
        };

        // One frame, or the difference means nothing.
        if let (Some(a), Some(b)) = (
            top.known.as_ref().and_then(|k| k.crs.as_ref()),
            base.known.as_ref().and_then(|k| k.crs.as_ref()),
        ) && a.uuid != b.uuid
        {
            return Err(Refusal::new(
                "frames",
                format!(
                    "`top` is in `{}` and `base` in `{}`: a difference of their heights would be a difference of two origins — bring them into one frame first",
                    a.title, b.title
                ),
            ));
        }
        let top_unit = top.known.as_ref().and_then(|k| k.vertical_uom.clone());
        let base_unit = base.known.as_ref().and_then(|k| k.vertical_uom.clone());
        if let (Some(a), Some(b)) = (&top_unit, &base_unit)
            && a != b
        {
            return Err(Refusal::new(
                "units",
                format!("`top` counts its heights in `{a}` and `base` in `{b}` — one unit, or no difference"),
            ));
        }
        let frame_unit = top_unit.or(base_unit);
        let uom = match (params.uom, &frame_unit) {
            (Some(given), _) => given,
            (None, Some(unit)) => unit.clone(),
            (None, None) => {
                return Err(Refusal::on(
                    "uom",
                    "is required: neither surface says the unit of its heights — name it, a thickness without one is a number",
                ));
            }
        };
        // Time is read off the frame when it says, off the answer otherwise.
        let family = Dimension::of(frame_unit.as_deref().unwrap_or(&uom));
        let time = family == Some(Dimension::Time);
        if let (Some(frame), Some(given)) = (family, Dimension::of(&uom))
            && frame != given
        {
            return Err(Refusal::on(
                "uom",
                format!(
                    "`{uom}` is a {given:?} and the frame counts its heights in a {frame:?} — the values would be read in the wrong family"
                ),
            ));
        }
        if time && measure == Measure::Stratigraphic {
            return Err(Refusal::on(
                "what",
                "an isopach needs lengths on all three axes: with heights in time the dip mixes metres and seconds, which is not an angle — ask for the isochore (an isochron), or convert to depth first",
            ));
        }

        let found = analysis::thickness(
            &support::Faces::of(&top.geometry).surface(),
            &support::Faces::of(&base.geometry).surface(),
            up,
            measure,
        )
        .map_err(|e| Refusal::new("refused", e.to_string()))?;
        cx.stop_if_cancelled()?;

        // The kind of the standard dictionary, looked up by title: the
        // dictionary is the authority for its own identifiers.
        let kind_title = match (measure, time) {
            (Measure::Vertical, false) => "true vertical thickness",
            (Measure::Vertical, true) => "vertical time thickness",
            (Measure::Stratigraphic, _) => "true stratigraphic thickness",
        };
        let kind = match params.property_kind {
            Some(given) => given.0,
            None => {
                let found = energyml_domain::property_kind::with_default(|dictionary| {
                    dictionary
                        .by_title(kind_title)
                        .first()
                        .map(|kind| (kind.uuid.clone(), kind.title.clone()))
                });
                let (uuid, title) = found.ok_or_else(|| {
                    Refusal::on(
                        "propertyKind",
                        format!("the dictionary in use holds no kind called `{kind_title}`: name one of the package"),
                    )
                })?;
                json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": title })
            }
        };
        let label = if time { "isochron" } else { params.what.word() };
        let title = match params.title {
            Some(given) => given.0,
            None => format!("{} — {} {label}", title_of(&top, "top"), title_of(&base, "base")),
        };

        // The field over the top, handed to the property writer **typed** —
        // with what is known of the top, whose node count it checks. The seed
        // is the one the derived bag's field gave before.
        let over = if top.known.is_some() {
            "top (object)"
        } else {
            "top"
        }
        .to_string();
        let count = found.values.len();
        let name = format!("{label} of top");
        let field = crate::declare::Field {
            name: name.clone(),
            over,
            on: "nodes".to_string(),
            values: found.values.clone(),
            uom: Some(uom.clone()),
        };
        let property = super::property::PropertyParams {
            representation: params.representation,
            property_kind: crate::declare::PropertyKindRef(kind),
            uom: uom.clone(),
            title: Title(title.clone()),
            data_file: params.data_file,
        };
        let mut plan = super::property::write(
            &field,
            top.known.as_ref(),
            &property,
            &format!("{name}:{title}"),
            cx,
        )?;

        plan.activity.recipe = "thickness";
        plan.activity.title = format!("thickness → {title}");
        plan.activity.parameters.push(Parameter::Text {
            title: "what".to_string(),
            value: params.what.word().to_string(),
        });
        plan.activity.parameters.push(Parameter::Text {
            title: "vertical".to_string(),
            value: params.vertical.word().to_string(),
        });
        let undefined = found.outside + found.folded + found.unplaced + found.undipped;
        plan.activity.parameters.push(Parameter::Double {
            title: "undefinedCount".to_string(),
            value: undefined as f64,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "crossedCount".to_string(),
            value: found.crossed as f64,
            uom: None,
        });

        let mut say = |key: &'static str, detail: String| plan.says.push(Note { key, detail });
        if measure == Measure::Stratigraphic {
            say(
                "parallel-beds",
                "the isopach is the vertical thickness times the cosine of the top's dip — exact where the beds run parallel, an approximation where they do not; the thickness along the normal waits for the ray caster"
                    .to_string(),
            );
        }
        if time {
            say(
                "isochron",
                format!(
                    "the heights are in `{uom}`: the difference is an isochron, written as `vertical time thickness`"
                ),
            );
        }
        if found.outside > 0 {
            say(
                "outside",
                format!(
                    "{} of {count} node(s) of the top have no base under them: NaN, not extrapolated",
                    found.outside
                ),
            );
        }
        if found.folded > 0 {
            say(
                "folded",
                format!(
                    "{} node(s) lie over several faces of the base that disagree — a fold or a reverse fault; which one is the base there is not a numerical choice: NaN",
                    found.folded
                ),
            );
        }
        if found.unplaced > 0 {
            say(
                "unplaced",
                format!(
                    "{} node(s) of the top have no finite position: NaN",
                    found.unplaced
                ),
            );
        }
        if found.undipped > 0 {
            say(
                "undipped",
                format!(
                    "{} node(s) of the top have no dip, so no isopach: NaN",
                    found.undipped
                ),
            );
        }
        if found.crossed > 0 {
            say(
                "crossed",
                format!(
                    "{} node(s) have the base above the top: their thickness is negative, and kept — two picks that cross",
                    found.crossed
                ),
            );
        }
        if found.set_aside > 0 {
            say(
                "set-aside",
                format!(
                    "{} face(s) of the base set aside: a corner that is not a finite point, no area in plan, or a cell that is not convex",
                    found.set_aside
                ),
            );
        }
        Ok(plan)
    }

    /// `what` and `vertical` are inputs, recorded under the parameters the
    /// projection already declares — a title cannot be both.
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
        vec![
            Slot::required("indexable", Need::Text, "the indexable element the values lie on"),
            count("valueCount", "values written"),
            count("undefinedCount", "nodes with no thickness, written as NaN"),
            count(
                "crossedCount",
                "nodes where the base lies above the top — negative, kept",
            ),
        ]
    }
}
