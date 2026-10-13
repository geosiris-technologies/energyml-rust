//! The styles the portions of a well with no strata information can be drawn in,
//! **declared** (several styles are offered: fixed colour, texture, dashes…, and the front end
//! must adapt dynamically to the styles proposed).
//!
//! The house rule - the operations' slots, the providers' filters - *what the
//! library declares **is** the front end's form*: [`declared`] lists each style, its
//! parameters and their kinds, and the front end builds a select and one control per
//! parameter from it, spelling no style and no parameter. A style added here
//! (a variant of [`UnstratifiedStyle`] and a [`StyleDecl`]) appears in the front end
//! with no page change.
//!
//! The form's answer **is** the style's JSON: `{ "kind": <id>, <param id>: <value>, … }`,
//! which `serde` reads into [`UnstratifiedStyle`]. A test holds the declared list
//! against the enum (defaults, ranges), so the two cannot drift.

use serde::{Deserialize, Serialize};

use crate::model::{NEUTRAL_COLOUR, UnstratifiedStyle, style_limits};

/// One style: what the user picks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleDecl {
    /// The `kind` of the style's JSON.
    pub id: String,
    /// The key the front end translates for the label (`sectionView.unstratified.style.dashed`).
    pub label_key: String,
    /// The label to show when the front end has no translation for the key.
    pub label: String,
    /// Its parameters, in the order the form shows them.
    pub params: Vec<ParamDecl>,
    /// The style a front end offers a **new** view - hatched
    /// texture for the portions without strata. The library's own default
    /// stays the first of the list (what a drawing has without asking: the CLI,
    /// the versioned drawings); this is the face's suggestion, said here so that
    /// the front end spells no style. One at most.
    #[serde(default)]
    pub suggested: bool,
}

/// One parameter of a style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamDecl {
    /// The field of the style's JSON.
    pub id: String,
    /// The key the front end translates for the label.
    pub label_key: String,
    /// The label to show when the front end has no translation for the key.
    pub label: String,
    /// What it is, and its default.
    #[serde(flatten)]
    pub kind: ParamKind,
}

/// What a parameter holds. The front end renders one control per kind; a kind it does
/// not know is shown as a plain text field of the default's JSON, never dropped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ParamKind {
    /// A colour (`#rrggbb`).
    Colour {
        /// Its default.
        default: String,
    },
    /// A number.
    Number {
        /// The unit, for the label (`px`, `°`).
        unit: String,
        /// The least value.
        min: f64,
        /// The greatest value.
        max: f64,
        /// The step of the control.
        step: f64,
        /// Its default.
        default: f64,
    },
    /// One of a list of words.
    Choice {
        /// The values, in the order the select shows them.
        values: Vec<ChoiceValue>,
        /// The default's `value`.
        default: String,
    },
}

/// One value of a [`ParamKind::Choice`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceValue {
    /// What the JSON holds.
    pub value: String,
    /// The key the front end translates for the label.
    pub label_key: String,
    /// The label to show when the front end has no translation for the key.
    pub label: String,
}

fn style(id: &str, label: &str, params: Vec<ParamDecl>) -> StyleDecl {
    StyleDecl {
        id: id.to_string(),
        label_key: format!("sectionView.unstratified.style.{id}"),
        label: label.to_string(),
        params,
        suggested: false,
    }
}

fn param(id: &str, label: &str, kind: ParamKind) -> ParamDecl {
    ParamDecl {
        id: id.to_string(),
        label_key: format!("sectionView.unstratified.param.{id}"),
        label: label.to_string(),
        kind,
    }
}

fn colour() -> ParamDecl {
    param(
        "colour",
        "Colour",
        ParamKind::Colour {
            default: NEUTRAL_COLOUR.to_string(),
        },
    )
}

fn length(id: &str, label: &str, default: f64) -> ParamDecl {
    let (min, max) = style_limits::LENGTH;
    param(
        id,
        label,
        ParamKind::Number {
            unit: "px".into(),
            min,
            max,
            step: 1.0,
            default,
        },
    )
}

/// The styles the portions with no strata can be drawn in, and their parameters.
/// The **first** is the default's (the one a drawing has without asking), so a
/// front end that starts from the list starts from what the library draws.
pub fn declared() -> Vec<StyleDecl> {
    let (smin, smax) = style_limits::SPACING;
    let (amin, amax) = style_limits::ANGLE;
    vec![
        style("solid", "Solid line", vec![colour()]),
        style(
            "dashed",
            "Dashed line",
            vec![colour(), length("dash", "Dash", 6.0), length("gap", "Gap", 4.0)],
        ),
        StyleDecl {
            suggested: true,
            ..style(
                "hatched",
                "Hatched texture",
                vec![
                    colour(),
                    param(
                        "spacing",
                        "Spacing",
                        ParamKind::Number {
                            unit: "px".into(),
                            min: smin,
                            max: smax,
                            step: 1.0,
                            default: 4.0,
                        },
                    ),
                    param(
                        "angle",
                        "Angle",
                        ParamKind::Number {
                            unit: "°".into(),
                            min: amin,
                            max: amax,
                            step: 5.0,
                            default: 45.0,
                        },
                    ),
                ],
            )
        },
        style("hidden", "Not drawn", Vec::new()),
    ]
}

impl StyleDecl {
    /// The style this declaration gives with every parameter at its default - the
    /// JSON the front end starts a choice from, read back through `serde`.
    pub fn default_style(&self) -> Result<UnstratifiedStyle, serde_json::Error> {
        let mut object = serde_json::Map::new();
        object.insert("kind".into(), serde_json::Value::String(self.id.clone()));
        for p in &self.params {
            let value = match &p.kind {
                ParamKind::Colour { default } => serde_json::Value::from(default.clone()),
                ParamKind::Number { default, .. } => serde_json::Value::from(*default),
                ParamKind::Choice { default, .. } => serde_json::Value::from(default.clone()),
            };
            object.insert(p.id.clone(), value);
        }
        serde_json::from_value(serde_json::Value::Object(object))
    }
}

/// Reads `id` or `id:param=value,param=value` (`dashed:colour=#999999,dash=6,gap=4`)
/// into a style, against [`declared`]: a parameter left out takes its default, an
/// unknown style or parameter is refused **naming what exists**. The command line
/// asks this; it holds no list of its own.
pub fn parse(text: &str) -> Result<UnstratifiedStyle, String> {
    let (id, rest) = text.split_once(':').map_or((text, ""), |(a, b)| (a, b));
    let styles = declared();
    let Some(style) = styles.iter().find(|s| s.id == id.trim()) else {
        let names: Vec<&str> = styles.iter().map(|s| s.id.as_str()).collect();
        return Err(format!(
            "unknown style `{}`: one of {}",
            id.trim(),
            names.join(", ")
        ));
    };
    let mut object = serde_json::Map::new();
    object.insert("kind".into(), serde_json::Value::String(style.id.clone()));
    for p in &style.params {
        let value = match &p.kind {
            ParamKind::Colour { default } | ParamKind::Choice { default, .. } => {
                serde_json::Value::from(default.clone())
            }
            ParamKind::Number { default, .. } => serde_json::Value::from(*default),
        };
        object.insert(p.id.clone(), value);
    }
    for pair in rest.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("`{pair}`: expected name=value"))?;
        let Some(p) = style.params.iter().find(|p| p.id == name.trim()) else {
            let names: Vec<&str> = style.params.iter().map(|p| p.id.as_str()).collect();
            return Err(format!(
                "style `{}` has no parameter `{}`: it has [{}]",
                style.id,
                name.trim(),
                names.join(", ")
            ));
        };
        let value = match &p.kind {
            ParamKind::Number { .. } => serde_json::Value::from(
                value
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("parameter `{}`: `{}` is not a number", p.id, value.trim()))?,
            ),
            ParamKind::Colour { .. } | ParamKind::Choice { .. } => {
                serde_json::Value::from(value.trim().to_string())
            }
        };
        object.insert(p.id.clone(), value);
    }
    let parsed: UnstratifiedStyle =
        serde_json::from_value(serde_json::Value::Object(object)).map_err(|why| why.to_string())?;
    match parsed.refusal() {
        Some(why) => Err(format!("style `{}`: {why}", style.id)),
        None => Ok(parsed),
    }
}
