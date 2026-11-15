//! The blocks several operations share, written once.
//!
//! `support` holds what every *representation* operation shares (the frame, the
//! slots, the objects). This module is its neighbour for the smaller blocks that
//! were pasted from one operation into the next - a refusal's wording, the mark
//! of an invented object, a door to the property-kind dictionary - each of which
//! drifted once the copies were edited one by one. One definition each: a rule
//! changed here changes for every operation, and a test of one operation covers
//! the rule for all.
//!
//! - [`digest`] - values as a uuid seed carries them;
//! - [`checked_description`], [`synthetic_extension`] - the 2000-character rule of
//!   a `Citation/Description` and the `synthetic` mark of an invented object
//!   (ADR-0019);
//! - [`names_frame`] - whether a named `crs` is the frame of the inputs;
//! - [`standard_kind`], [`standard_kind_dor`] - the one entrance to the
//!   property-kind dictionary by title;
//! - [`refuse_degrees`], [`refuse_degrees_target`] - a vertical extent is not
//!   written in a frame in degrees;
//! - [`ring_corners`] - a ring's corners in plan;
//! - [`triangle_surface`] - a shape's triangles as one surface;
//! - [`wellbore_pair`] - a wellbore feature and its interpretation.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::crs::FrameKind;
use crate::declare::{CrsRef, Shape};
use crate::operation::{NewObject, Refusal};

use super::support::Frame;

/// Values as a seed carries them - exactly (`{:?}` reads back the same double),
/// NaN included, comma-joined.
///
/// The text is the seed of the uuids the operations that lay values draw, so it
/// is **not** to be changed without changing every uuid they have written. Built
/// in one string: a million-value property is about 20 MB of text.
pub fn digest(values: &[f64]) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The most characters an eml `Citation/Description` holds.
const DESCRIPTION_LIMIT: usize = 2000;

/// A description a `Citation` can hold, or a refusal of `source`.
///
/// Refused rather than cut short, since a source cut short cites nothing.
pub fn checked_description(description: &str) -> Result<(), Refusal> {
    let length = description.chars().count();
    if length > DESCRIPTION_LIMIT {
        return Err(Refusal::on(
            "source",
            format!(
                "makes a description of {length} characters, and a Citation/Description holds {DESCRIPTION_LIMIT} - cite the document shorter"
            ),
        ));
    }
    Ok(())
}

/// The `ExtensionNameValue` an invented object carries - `synthetic = true`,
/// and `by` saying what laid it (the reference-model rule of ADR-0019).
pub fn synthetic_extension(by: impl Into<String>) -> Value {
    let by: String = by.into();
    json!([{
        "$type": "eml23.ExtensionNameValue",
        "Name": "synthetic",
        "Value": { "$type": "eml23.StringMeasure", "_": "true" },
        "Description": by,
    }])
}

/// Whether the `crs` a caller named is the frame with this uuid.
///
/// Case-insensitive, as a uuid is: the one rule every operation that lays its
/// result in its inputs' frame applies (it was case-sensitive in `project`).
pub fn names_frame(named: &CrsRef, uuid: &str) -> bool {
    let named = named
        .0
        .get("Uuid")
        .and_then(|value| value.as_str())
        .or_else(|| named.0.as_str())
        .unwrap_or_default();
    named.eq_ignore_ascii_case(uuid)
}

/// The kind the standard dictionary holds under `title` - its uuid and its
/// own title - or `None`. The one entrance to the dictionary by title: the
/// dictionary is the authority for its own identifiers, and a uuid typed into
/// an operation would be a second copy of one.
pub fn standard_kind(title: &str) -> Option<(String, String)> {
    energyml_domain::property_kind::with_default(|dictionary| {
        dictionary
            .by_title(title)
            .first()
            .map(|kind| (kind.uuid.clone(), kind.title.clone()))
    })
}

/// [`standard_kind`] as the `DataObjectReference` a property writes, or a
/// refusal of `slot`.
pub fn standard_kind_dor(title: &str, slot: &str) -> Result<Value, Refusal> {
    let (uuid, named) = standard_kind(title).ok_or_else(|| {
        Refusal::on(
            slot,
            format!(
                "the dictionary in use holds no kind called `{title}`: name a `propertyKind` of the package"
            ),
        )
    })?;
    Ok(
        json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": named }),
    )
}

/// A vertical extent (`subject`: `a wellbore`, `a vertical polyline`) is refused
/// in a frame in degrees: its lengths are linear, the frame's horizontal axes
/// are not, and every `thing` would come out between a hundred and ten thousand
/// times longer than the model is wide. Refused by the slot to change - `crs`
/// when one was named, `project` when the frame was created without it.
pub fn refuse_degrees(frame: &Frame, named: bool, subject: &str, thing: &str) -> Result<(), Refusal> {
    if frame.kind != Some(FrameKind::Geographic) {
        return Ok(());
    }
    let (slot, what) = if named {
        (
            "crs",
            format!(
                "the frame `{}` named",
                frame.crs["Title"].as_str().unwrap_or_default()
            ),
        )
    } else {
        ("project", "the frame created without `project`".to_string())
    };
    Err(Refusal::on(
        slot,
        format!(
            "{subject} is a vertical extent, and {what} measures its horizontal axes in degrees while its lengths are linear: every {thing} would be written between a hundred and ten thousand times longer than the model is wide, which no reader can reconcile - name a projected system in `project` (EPSG:2154 for mainland France), or a LocalEngineeringCompoundCrs as `crs`"
        ),
    ))
}

/// [`refuse_degrees`] for a frame still to be created, known by its EPSG code:
/// a borehole is refused in `EPSG:code` when that is a system in degrees.
pub fn refuse_degrees_target(code: impl std::fmt::Display) -> Refusal {
    Refusal::new(
        "project",
        format!(
            "a borehole is a vertical extent, and EPSG:{code} measures its horizontal axes in degrees while its depths are linear: every hole would be written between a hundred and ten thousand times longer than the model is wide, which no reader can reconcile - name a projected system in `project` (EPSG:2154 for mainland France)"
        ),
    )
}

/// A ring's corners in plan, flat `[x, y, ...]` - refused by `slot`'s name when
/// it cannot enclose anything (fewer than 3 points).
pub fn ring_corners(slot: &str, ring: &Shape) -> Result<Vec<f64>, Refusal> {
    let points: Vec<[f64; 3]> = ring
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().copied())
        .collect();
    if points.len() < 3 {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` holds {} point(s): a ring encloses something with at least 3",
                ring.title(),
                points.len()
            ),
        ));
    }
    Ok(points.iter().flat_map(|p| [p[0], p[1]]).collect())
}

/// A triangulated surface: its nodes and its triangles.
pub type Surface = (Vec<[f64; 3]>, Vec<[u32; 3]>);

/// A shape's triangles, every patch's indices shifted to the whole shape's
/// nodes - or `None` when it holds none.
pub fn triangle_surface(shape: &Shape) -> Option<Surface> {
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for patch in &shape.geometry.patches {
        let Topology::Triangles(faces) = &patch.topology else {
            continue;
        };
        let base = points.len() as u32;
        points.extend(patch.points.iter().copied());
        triangles.extend(faces.iter().map(|f| [f[0] + base, f[1] + base, f[2] + base]));
    }
    (!triangles.is_empty()).then_some((points, triangles))
}

/// What a wellbore feature and its interpretation say beyond their identity.
///
/// `None` writes nothing: a table of heads says neither `IsWellKnown` nor
/// `IsDrilled`, and what is not known is not written.
#[derive(Default)]
pub struct WellboreFlags<'a> {
    /// `IsWellKnown` of the feature.
    pub is_well_known: Option<bool>,
    /// `IsDrilled` of the interpretation - `false` for a planned or invented
    /// hole (Wells.xsd: "for planned trajectories, use IsDrilled=FALSE"), `true`
    /// for a hole the producer states was drilled.
    pub is_drilled: Option<bool>,
    /// More members of the feature - aliases, extension values.
    pub feature_extras: Vec<(&'a str, Value)>,
}

/// The wellbore feature and its interpretation - the two objects every
/// wellbore-writing operation adds when none is named - and the
/// `DataObjectReference` of the interpretation.
///
/// The identities stay the caller's (their seeds differ: a producer's own uuid,
/// a derived one), and so does the citation; what was written five times is the
/// JSON of the pair, and the flags that differ on purpose are [`WellboreFlags`].
pub fn wellbore_pair(
    feature_uuid: &str,
    feature_title: &str,
    interpretation_uuid: &str,
    interpretation_title: &str,
    cite: impl Fn(&str) -> Value,
    flags: WellboreFlags<'_>,
) -> (Vec<NewObject>, Value) {
    let mut feature_json = json!({
        "$type": "resqml22.WellboreFeature",
        "uuid": feature_uuid,
        "schemaVersion": "2.2",
        "Citation": cite(feature_title),
    });
    if let Some(known) = flags.is_well_known {
        feature_json["IsWellKnown"] = json!(known);
    }
    for (name, value) in flags.feature_extras {
        feature_json[name] = value;
    }
    let mut interpretation_json = json!({
        "$type": "resqml22.WellboreInterpretation",
        "uuid": interpretation_uuid,
        "schemaVersion": "2.2",
        "Citation": cite(interpretation_title),
        "Domain": "depth",
        "InterpretedFeature": {
            "$type": "eml23.DataObjectReference",
            "Uuid": feature_uuid,
            "QualifiedType": "resqml22.WellboreFeature",
            "Title": feature_title,
        },
    });
    if let Some(drilled) = flags.is_drilled {
        interpretation_json["IsDrilled"] = json!(drilled);
    }
    let objects = vec![
        NewObject {
            uuid: feature_uuid.to_string(),
            qualified_type: "resqml22.WellboreFeature".to_string(),
            role: "feature".to_string(),
            title: feature_title.to_string(),
            json: feature_json,
        },
        NewObject {
            uuid: interpretation_uuid.to_string(),
            qualified_type: "resqml22.WellboreInterpretation".to_string(),
            role: "interpretation".to_string(),
            title: interpretation_title.to_string(),
            json: interpretation_json,
        },
    ];
    let reference = json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": interpretation_uuid,
        "QualifiedType": "resqml22.WellboreInterpretation",
        "Title": interpretation_title,
    });
    (objects, reference)
}

/// A number as the activity records it - a parameter without a unit.
pub fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    }
}
