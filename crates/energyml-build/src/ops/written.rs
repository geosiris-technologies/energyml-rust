//! What every writer asks — the frame, what is represented, the naming —
//! **declared once, as types**.
//!
//! These structures are the only declaration of those slots. The operations
//! not yet migrated still reach them through [`super::support::common_slots`],
//! which is now these slots with [`suggest_written`]'s suggestions on them; an
//! operation written against typed parameters embeds [`Written`] (or
//! [`WrittenImposed`], when it imposes what it represents) with
//! `#[param(flatten)]`.
//!
//! # Required for writing, absent for a geometry
//!
//! `title` is required — a written object needs one — but a plan asked for its
//! **geometry only** may leave it empty: the settling relaxes what
//! is outside the `operation` group. So the field is an `Option` declared
//! `least = 1`: the form says *required*, the reading tolerates the absence,
//! and the writer answers `untitled`, as it always has.

use serde_json::json;

use crate::declare::{CrsRef, Epsg, InterpretationRef, Params, Suggestions, Title, TypeName};
use crate::operation::{Need, Otherwise};
use crate::pieces::{Pieces, Ref};
use crate::types::TypeChoice;

/// Where the coordinates are: a CRS of the package, or one created here from
/// EPSG codes — two alternatives, and they say so.
#[derive(Debug, Clone, Params)]
pub struct FrameParams {
    /// the frame the coordinates are already in — a LocalEngineeringCompoundCrs of the package (a ProjectedCrs is taken, with a note); or leave it out and give `sourceEpsg`
    ///
    /// **A whole system, never a half**: what it names is judged by
    /// `crate::crs::frame_kind` in `support::frame_in`, and a `VerticalCrs`, a
    /// `LocalEngineering2dCrs` or a reference point blocks the writing by its
    /// name. The list a form offers is `crate::crs::FRAMES`.
    #[param(without = ["sourceEpsg"], group = "frame", reads = ["Geometry/LocalCrs", "Patches/0/Geometry/LocalCrs", "LocalCrs"])]
    pub crs: Option<CrsRef>,
    /// the EPSG code the coordinates are in, when no `crs` of the package is named: the frame is then created here — a LocalEngineeringCompoundCrs over a ProjectedCrs with `project`, a Geographic2dCrs without
    ///
    /// **What this slot declares it creates must be what it creates.** It announced
    /// an `eml23.LocalEngineeringCompoundCrs` that `crs::built` did not write
    /// (measured on an import of 936 boreholes), and the declaration had
    /// been brought back to what was written. Since then the compound **is**
    /// written, with its `LocalEngineering2dCrs` — RESQML TRG 2.2 §3.1 p. 22 —,
    /// and it is therefore back in the list, first: it is what a geometry
    /// names.
    ///
    /// `Otherwise::Creates` exists precisely so that a slot **names what
    /// it creates**; a declaration that names something else is worse than an
    /// absence — the form promises an object that the package will never have, and
    /// nobody will check.
    ///
    /// Both cases are named because the answer depends on `project`,
    /// which a static declaration cannot read: four objects in metres,
    /// two in degrees (the `Geographic2dCrs` and the `VerticalCrs`, the named
    /// exception).
    #[param(
        without = ["crs"],
        group = "frame",
        creates = [
            "eml23.LocalEngineeringCompoundCrs",
            "eml23.LocalEngineering2dCrs",
            "eml23.ProjectedCrs",
            "eml23.VerticalCrs",
            "eml23.Geographic2dCrs"
        ]
    )]
    pub source_epsg: Option<Epsg>,
    /// with `sourceEpsg`: reproject into this EPSG system, in metres
    #[param(with = ["sourceEpsg"], group = "frame")]
    pub project: Option<Epsg>,
    /// with `sourceEpsg`: the EPSG code of the vertical system, when known
    #[param(with = ["sourceEpsg"], group = "frame")]
    pub vertical: Option<Epsg>,
}

/// What the representation represents, when somebody names it.
#[derive(Debug, Clone, Params)]
pub struct RepresentedParams {
    /// what the representation represents — an interpretation of the package
    #[param(group = "represents", reads = ["RepresentedObject", "RepresentedInterpretation"])]
    pub interpretation: Option<InterpretationRef>,
}

/// The title and the file of what is written.
#[derive(Debug, Clone, Params)]
pub struct NamingParams {
    /// the representation's title
    #[param(group = "naming", least = 1)]
    pub title: Option<Title>,
    /// the external file the arrays go into
    #[param(default = String::from("data.h5"), group = "naming")]
    pub data_file: String,
}

/// The feature and interpretation to create, for an operation that does not
/// impose them — chosen from the schema's list, the package and the version
/// **fixed** because these operations write RESQML 2.2.
#[derive(Debug, Clone, Params)]
pub struct NewFeatureParams {
    /// the feature to create when no interpretation is named — what this represents in the ground
    #[param(
        without = ["interpretation"],
        group = "represents",
        need = Need::Type(TypeChoice::filtered("feature").only_in(&["resqml"], &["2.2"]))
    )]
    pub feature_type: Option<TypeName>,
    /// the interpretation to create over that feature; left out, a generic one
    #[param(
        default = TypeName(String::from("resqml22.GenericFeatureInterpretation")),
        optional,
        with = ["featureType"],
        group = "represents",
        need = Need::Type(TypeChoice::filtered("interpretation").only_in(&["resqml"], &["2.2"]))
    )]
    pub interpretation_type: TypeName,
    /// the kind, for a feature type that has one (a cultural feature); a type without one ignores it
    #[param(with = ["featureType"], group = "represents")]
    pub feature_kind: Option<String>,
}

/// Everything a generic representation writer asks — the operation imposing
/// nothing it represents.
#[derive(Debug, Clone, Params)]
pub struct Written {
    /// Where the coordinates are.
    #[param(flatten)]
    pub frame: FrameParams,
    /// What is represented, when named.
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
    /// The feature to create when nothing is named.
    #[param(flatten)]
    pub new_feature: NewFeatureParams,
}

/// The same, for an operation that **imposes** what it represents (`well`,
/// `boundary`): no feature type to choose.
#[derive(Debug, Clone, Params)]
pub struct WrittenImposed {
    /// Where the coordinates are.
    #[param(flatten)]
    pub frame: FrameParams,
    /// What is represented, when named — created here otherwise.
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

impl NamingParams {
    /// The title, or `untitled` on a plan asked for its geometry — the
    /// writing refuses a missing one before this is ever reached.
    pub fn title(&self) -> String {
        self.title
            .as_ref()
            .map(|title| title.0.clone())
            .unwrap_or_else(|| "untitled".to_string())
    }
}

/// What the inputs suggest for the shared slots — posed on the declared ones.
///
/// # What is read, and why each
///
/// - **the CRS**, when the inputs name exactly one — **deduplicated by uuid**,
///   because an answered slot comes back into the bag as a piece of its own:
///   the CRS an input named and the CRS somebody just picked are then the same
///   system seen twice, and the suggestion — and the line saying where it came
///   from — would vanish the moment the person agreed with it;
/// - **4326 for `sourceEpsg`** when a GeoJSON is there and no CRS is known: a
///   GeoJSON is WGS 84 by definition (RFC 7946), a shape is not;
/// - **the codes an input's own frame names**, so a newcomer lands beside what
///   is already there — only when the frame *names* one: a
///   `ProjectedUnknownCrs` says the producer does not know;
/// - **the interpretation**, when the inputs name exactly one;
/// - **a title**, from the first named piece;
/// - and, when the operation **imposes** what it represents, that the
///   interpretation left out is **created** — an optional field reads *you
///   may do without*, not *I make it for you*.
pub fn suggest_written(
    pieces: &Pieces,
    represents: Option<(&'static str, &'static str)>,
    slots: &mut Suggestions<'_>,
) {
    let has_features = pieces.features().next().is_some();
    let mut known_crs: Vec<&Ref> = Vec::new();
    for one in pieces.objects().filter_map(|(_, known)| known.crs.as_ref()) {
        if !known_crs
            .iter()
            .any(|seen| seen.uuid.eq_ignore_ascii_case(&one.uuid))
        {
            known_crs.push(one);
        }
    }
    if let [one] = known_crs.as_slice() {
        slots.inferred("crs", one.dor(), (*one).clone());
    }
    if has_features && known_crs.is_empty() {
        slots.set("sourceEpsg", json!(4326));
    }
    if let Some((known, code)) = pieces
        .objects()
        .find_map(|(_, k)| k.crs_epsg.map(|code| (k, code)))
    {
        match known.crs.as_ref() {
            Some(one) => slots.inferred("project", json!(code), one.clone()),
            None => {
                slots.set("project", json!(code));
            }
        }
    }
    if let Some((known, code)) = pieces
        .objects()
        .find_map(|(_, k)| k.vertical_epsg.map(|code| (k, code)))
    {
        match known.vertical_crs.as_ref().or(known.crs.as_ref()) {
            Some(one) => slots.inferred("vertical", json!(code), one.clone()),
            None => {
                slots.set("vertical", json!(code));
            }
        }
    }
    // **An operation that imposes what it represents is suggested only that
    // kind of interpretation.** The one input naming an interpretation may be
    // a horizon's polyline or the ground a wellhead is read on: a well then
    // represented the horizon (24/09, `well-from-plan` in Lutetia; the same
    // path in `well-from-polyline`). Compared on the bare class, as
    // `Takes::accepts` compares types.
    let bare = |qualified: &str| {
        let class = qualified
            .rsplit('.')
            .next()
            .unwrap_or(qualified)
            .to_ascii_lowercase();
        class.strip_prefix("obj_").map(str::to_string).unwrap_or(class)
    };
    let imposed = represents.map(|(_, interpretation)| bare(interpretation));
    let mut known_interpretation: Vec<&Ref> = Vec::new();
    for one in pieces
        .objects()
        .filter_map(|(_, known)| known.interpretation.as_ref())
        .filter(|one| {
            imposed
                .as_ref()
                .is_none_or(|wanted| bare(&one.qualified_type) == *wanted)
        })
    {
        if !known_interpretation
            .iter()
            .any(|seen| seen.uuid.eq_ignore_ascii_case(&one.uuid))
        {
            known_interpretation.push(one);
        }
    }
    // `interpretation` is left out by an operation that makes its own
    // (`boundary`): the shared suggestion then has nowhere to go, and that is
    // not a typo to fail on.
    if let [one] = known_interpretation.as_slice()
        && slots.declared("interpretation")
    {
        slots.inferred("interpretation", one.dor(), (*one).clone());
    }
    if let Some((feature, interpreted)) = represents
        && slots.declared("interpretation")
        && let Some(slot) = slots.slot_mut("interpretation")
    {
        slot.about = "what the representation represents — an interpretation of the package; left out, it is created here with its feature";
        slot.otherwise = Otherwise::Creates {
            types: vec![feature, interpreted],
        };
    }
    // Left out too by an operation that titles its objects itself
    // (`contacts-from-frames`: one title per unit and side).
    if let Some(name) = super::support::suggested_title(pieces)
        && slots.declared("title")
    {
        slots.set("title", json!(name));
    }
}
