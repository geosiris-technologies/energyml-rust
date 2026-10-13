//! The markers a section can carry, **declared** — the shapes read from the
//! model, the places listed (ADR-0015).
//!
//! The house rule, as for [`crate::style::declared`]: *what the library
//! declares is the front end's form*. [`declared`] lists every shape a marker can
//! take and every place it can sit at, each with a readable label and a
//! translation key; the front end builds its chooser from the list and spells no
//! value.
//!
//! # The shapes are not typed here
//!
//! RESQML already has a vocabulary of symbols: `GraphicalInformationForNodes`
//! has a `Symbol` field whose enumeration lists them (`circle`, `cross`, `cube`,
//! `diamond`, `plus`, `point`, `pyramid`, `sphere`, `star`, `tetrahedron`). They
//! are read **by reflection**, from every release the registry holds
//! (`FieldType::allowed_values`), and each shape remembers the pairs
//! *(qualified type of the enumeration, value)* that write it — what an
//! attachment of the marker to a package would write. A release that
//! adds a value brings the shape with no line here; what it cannot bring is the
//! way to **draw** it, so a value [`is_drawn`] does not know is listed with
//! `drawn: false` and refused by name when a marker asks for it — never a
//! default circle. [`CONTOUR`] is the tool's own shape (a wide line under the
//! well), with no pair.

use energyml_core::naming;
use energyml_core::reflect::TypeKind;
use energyml_core::registry::Registry;
use serde::{Deserialize, Serialize};

/// The type whose `Symbol` field holds the shapes.
const SYMBOL_OWNER: &str = "GraphicalInformationForNodes";
/// Its field.
const SYMBOL_FIELD: &str = "Symbol";

/// The tool's own shape: a wide coloured line under the object, the
/// highlight with a name and a colour.
pub const CONTOUR: &str = "contour";

/// The places a marker can sit at, in the order the form lists them.
pub const PLACES: [&str; 3] = ["head", "bottom", "md"];

/// The shapes the drawer knows how to write — the one list the drawing and
/// [`DeclaredShape::drawn`] both read.
const DRAWN: [&str; 11] = [
    "circle",
    "cross",
    "cube",
    "diamond",
    "plus",
    "point",
    "pyramid",
    "sphere",
    "star",
    "tetrahedron",
    CONTOUR,
];

/// Whether the drawer knows this shape: a value the model lists that is not here
/// is declared and refused by its name.
pub fn is_drawn(id: &str) -> bool {
    DRAWN.contains(&id)
}

/// One shape a marker can take.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredShape {
    /// What a marker's `shape` holds: the model's value (`star`), or [`CONTOUR`].
    pub id: String,
    /// A readable label, in English.
    pub label: String,
    /// The key the front end translates for the label (`sectionView.markers.shape.star`).
    pub label_key: String,
    /// Every `(qualified type of the enumeration, value)` the model writes it
    /// as, one per release that has it (`resqml22.NodeSymbolExt`, `star`).
    /// Empty for the tool's own shapes.
    pub format: Vec<(String, String)>,
    /// Whether this build draws it.
    pub drawn: bool,
}

/// One place a marker can sit at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredPlace {
    /// The `kind` of the place's JSON (`MarkerPlace`).
    pub id: String,
    /// A readable label, in English.
    pub label: String,
    /// The key the front end translates for the label (`sectionView.markers.place.md`).
    pub label_key: String,
    /// Whether the place carries a number (`value`, in the well's MD unit).
    pub takes_value: bool,
}

/// The shapes the registry's releases offer, and the places — what the front end's
/// form is built from.
///
/// Shapes come in the order the registry met them, once each however many
/// releases write them, and [`CONTOUR`] last.
pub fn declared(registry: &Registry) -> (Vec<DeclaredShape>, Vec<DeclaredPlace>) {
    let mut shapes: Vec<DeclaredShape> = Vec::new();
    for info in registry
        .types()
        .filter(|t| t.kind == TypeKind::Struct && t.name == SYMBOL_OWNER)
    {
        let Some(field) = info.field(SYMBOL_FIELD) else {
            continue;
        };
        let Some(enumeration) = field.ty.type_info() else {
            continue;
        };
        let qualified = format!(
            "{}{}.{}",
            info.package,
            naming::compact_version(naming::release_of(enumeration.version)),
            enumeration.name
        );
        for variant in field.ty.allowed_values().iter().filter(|v| !v.is_other) {
            let pair = (qualified.clone(), variant.value.to_string());
            match shapes.iter_mut().find(|s| s.id == variant.value) {
                Some(shape) => {
                    if !shape.format.contains(&pair) {
                        shape.format.push(pair);
                    }
                }
                None => shapes.push(shape(variant.value, vec![pair])),
            }
        }
    }
    shapes.push(shape(CONTOUR, Vec::new()));
    let places = PLACES
        .iter()
        .map(|&id| DeclaredPlace {
            id: id.to_string(),
            label: match id {
                "head" => "Top of the well",
                "bottom" => "Bottom of the well",
                _ => "At a measured depth",
            }
            .to_string(),
            label_key: format!("sectionView.markers.place.{id}"),
            takes_value: id == "md",
        })
        .collect();
    (shapes, places)
}

fn shape(id: &str, format: Vec<(String, String)>) -> DeclaredShape {
    let label = if id == CONTOUR {
        "Contour (a wide line under the object)".to_string()
    } else {
        let mut chars = id.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    };
    DeclaredShape {
        id: id.to_string(),
        label,
        label_key: format!("sectionView.markers.shape.{id}"),
        format,
        drawn: is_drawn(id),
    }
}

/// The size of a symbol when nothing says otherwise, pixels.
pub const DEFAULT_SIZE_PX: f64 = 12.0;

/// A marker asked for in words, against the objects of a catalogue — what the
/// command line's `--marker` gives (08/10, Lutetia's BSS wells).
///
/// `NAME:shape=diamond,colour=#1565c0,at=bottom,type=WellboreTrajectory,title=BSS`.
/// `shape`, `colour` and `at` (`head`, `bottom`, `md` with `md=VALUE`) are
/// required; `size` defaults to [`DEFAULT_SIZE_PX`]. The objects are the
/// catalogue's rows `shown_in` that section — drawn there, or merged into a
/// line drawn there; `None`: on any section — whose qualified type contains
/// `type` (several joined by
/// `|`, any of them) and whose title contains `title`, case ignored — the
/// storage's own [`type_matches`](energyml_storage::resource::type_matches) and
/// [`title_matches`](energyml_storage::resource::title_matches). At least one
/// of the two is asked for, and a marker that matches no object is refused: a
/// legend entry for nothing reads like a symbol that failed to draw.
pub fn from_spec(
    text: &str,
    rows: &[crate::model::ObjectRow],
    shown_in: Option<crate::model::SectionKey>,
) -> Result<crate::model::Marker, String> {
    use crate::model::{Marker, MarkerPlace, ObjectRow, Outcome};
    use energyml_storage::resource::{title_matches, type_matches};

    let (name, rest) = text
        .split_once(':')
        .ok_or_else(|| format!("`{text}`: expected NAME:shape=…,colour=…,at=…"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("`{text}`: a marker needs a name, the legend shows it"));
    }
    let (mut shape, mut colour, mut at, mut md, mut size, mut types, mut title) =
        (None, None, None, None, None, None, None);
    for pair in rest.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("`{pair}`: expected name=value"))?;
        let value = value.trim().to_string();
        match key.trim() {
            "shape" => shape = Some(value),
            "colour" | "color" => colour = Some(value),
            "at" => at = Some(value),
            "md" => {
                md = Some(
                    value
                        .parse::<f64>()
                        .map_err(|_| format!("`md={value}` is not a number"))?,
                )
            }
            "size" => {
                size = Some(
                    value
                        .parse::<f64>()
                        .map_err(|_| format!("`size={value}` is not a number"))?,
                )
            }
            "type" => types = Some(value),
            "title" => title = Some(value),
            other => {
                return Err(format!(
                    "a marker has no parameter `{other}`: it has [shape, colour, at, md, size, type, title]"
                ));
            }
        }
    }
    let shape = shape.ok_or("a marker needs `shape=`")?;
    if !is_drawn(&shape) {
        return Err(format!(
            "shape `{shape}` is not one this build draws: one of {}",
            DRAWN.join(", ")
        ));
    }
    let colour = colour.ok_or("a marker needs `colour=` (#rrggbb or #rrggbbaa)")?;
    let hex = colour.strip_prefix('#').unwrap_or("");
    if !matches!(hex.len(), 6 | 8) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("colour `{colour}`: expected #rrggbb or #rrggbbaa"));
    }
    let at = match (at.as_deref(), md) {
        (Some("head"), None) => MarkerPlace::Head,
        (Some("bottom"), None) => MarkerPlace::Bottom,
        (Some("md"), Some(value)) => MarkerPlace::Md { value },
        (Some("md"), None) => return Err("`at=md` needs `md=VALUE`, in the well's MD unit".into()),
        (Some("head" | "bottom"), Some(_)) => return Err("`md=` goes with `at=md` only".into()),
        (Some(other), _) => return Err(format!("`at={other}`: one of {}", PLACES.join(", "))),
        (None, _) => return Err(format!("a marker needs `at=`: one of {}", PLACES.join(", "))),
    };
    let size_px = size.unwrap_or(DEFAULT_SIZE_PX);
    if !(size_px.is_finite() && size_px > 0.0) {
        return Err(format!("`size={size_px}`: a size is a positive number of pixels"));
    }
    if types.is_none() && title.is_none() {
        return Err("a marker names its objects: give `type=` or `title=` (or both)".into());
    }
    // What the section shows: drawn there, or folded into the line of another
    // object drawn there (the marker lands on that line). A refused or missed
    // object, or one drawn on another section only, has no place in the drawing
    // — naming it would only add a note that nothing is there.
    let drawn_in = |row: &ObjectRow| match &row.outcome {
        Outcome::Drawn { sections } => shown_in.is_none_or(|key| sections.contains(&key)),
        _ => false,
    };
    let shown = |row: &ObjectRow| match &row.outcome {
        Outcome::Merged { into } => rows.iter().any(|other| other.object == *into && drawn_in(other)),
        _ => drawn_in(row),
    };
    let keys: Vec<String> = rows
        .iter()
        .filter(|row| shown(row))
        .filter(|row| {
            types.as_deref().is_none_or(|types| {
                types
                    .split('|')
                    .any(|one| type_matches(&row.object.qualified_type, one.trim()))
            }) && title
                .as_deref()
                .is_none_or(|title| title_matches(&row.object.title, title))
        })
        .map(|row| row.object.key())
        .collect();
    if keys.is_empty() {
        return Err(format!("marker `{name}` matches no object of the section"));
    }
    Ok(Marker {
        name: name.to_string(),
        shape,
        colour,
        at,
        keys,
        size_px,
        hidden: false,
    })
}
