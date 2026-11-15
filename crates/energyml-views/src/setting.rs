//! The settings a view declares ([`ViewSetting`]) and the settings of one request, checked and completed with
//! their defaults ([`Settled`]).
//!
//! A setting is declared once, by the view: its name, its nature, its default, its bounds, its sentence. The door
//! checks what a request gives against it - an unknown name, a value of the wrong nature or out of its bounds is
//! refused by its name - and fills the defaults, so that a view reads only settled values and a drawing records
//! every value it was drawn with.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::refusal::ViewRefusal;

/// The nature of a setting.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingKind {
    /// A number, in a unit when it has one, within its bounds.
    Number {
        /// The unit the number is read in (`m`, `dega`); `None`: no unit.
        uom: Option<&'static str>,
        /// The least value accepted.
        least: Option<f64>,
        /// The greatest value accepted.
        most: Option<f64>,
    },
    /// A whole number, within its bounds.
    Integer {
        /// The least value accepted.
        least: Option<i64>,
        /// The greatest value accepted.
        most: Option<i64>,
    },
    /// One word among a list.
    Choice(&'static [&'static str]),
    /// `true` or `false`.
    Flag,
    /// Any text.
    Text,
    /// Any JSON value the view reads itself (a list, a pick); written as JSON on a command line.
    Json,
}

/// One setting a view reads.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSetting {
    /// Its name, `snake_case`.
    pub name: &'static str,
    /// Its nature.
    pub kind: SettingKind,
    /// Its default; `None` with `required` false: absent unless given.
    pub default: Option<Value>,
    /// Whether a request must give it (and no default stands in).
    pub required: bool,
    /// What it is, in one sentence.
    pub about: &'static str,
}

impl ViewSetting {
    fn of(name: &'static str, kind: SettingKind, about: &'static str) -> Self {
        ViewSetting {
            name,
            kind,
            default: None,
            required: false,
            about,
        }
    }

    /// A number with no unit and no bounds.
    pub fn number(name: &'static str, about: &'static str) -> Self {
        Self::of(
            name,
            SettingKind::Number {
                uom: None,
                least: None,
                most: None,
            },
            about,
        )
    }

    /// A whole number with no bounds.
    pub fn integer(name: &'static str, about: &'static str) -> Self {
        Self::of(
            name,
            SettingKind::Integer {
                least: None,
                most: None,
            },
            about,
        )
    }

    /// One word among `words`.
    pub fn choice(name: &'static str, words: &'static [&'static str], about: &'static str) -> Self {
        Self::of(name, SettingKind::Choice(words), about)
    }

    /// `true` or `false`, `false` by default.
    pub fn flag(name: &'static str, about: &'static str) -> Self {
        Self::of(name, SettingKind::Flag, about).default(Value::Bool(false))
    }

    /// Any text.
    pub fn text(name: &'static str, about: &'static str) -> Self {
        Self::of(name, SettingKind::Text, about)
    }

    /// A JSON value the view reads itself.
    pub fn json(name: &'static str, about: &'static str) -> Self {
        Self::of(name, SettingKind::Json, about)
    }

    /// The unit a number is read in.
    pub fn uom(mut self, uom: &'static str) -> Self {
        if let SettingKind::Number { uom: unit, .. } = &mut self.kind {
            *unit = Some(uom);
        }
        self
    }

    /// The bounds of a number or a whole number (a whole number takes them rounded towards the inside).
    pub fn bounds(mut self, least: Option<f64>, most: Option<f64>) -> Self {
        match &mut self.kind {
            SettingKind::Number {
                least: low,
                most: high,
                ..
            } => {
                *low = least;
                *high = most;
            }
            SettingKind::Integer {
                least: low,
                most: high,
            } => {
                *low = least.map(|value| value.ceil() as i64);
                *high = most.map(|value| value.floor() as i64);
            }
            _ => {}
        }
        self
    }

    /// Its default.
    pub fn default(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    /// A request must give it.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Its nature, in words: `number (m) in [0, ∞)`, `one of md_bar, plan`...
    pub fn nature(&self) -> String {
        let bound = |value: Option<String>| value.unwrap_or_else(|| "∞".to_string());
        match &self.kind {
            SettingKind::Number { uom, least, most } => {
                let unit = uom.map(|uom| format!(" ({uom})")).unwrap_or_default();
                if least.is_none() && most.is_none() {
                    format!("number{unit}")
                } else {
                    format!(
                        "number{unit} in [{}, {}]",
                        bound(least.map(|v| v.to_string())),
                        bound(most.map(|v| v.to_string()))
                    )
                }
            }
            SettingKind::Integer { least, most } => {
                if least.is_none() && most.is_none() {
                    "whole number".to_string()
                } else {
                    format!(
                        "whole number in [{}, {}]",
                        bound(least.map(|v| v.to_string())),
                        bound(most.map(|v| v.to_string()))
                    )
                }
            }
            SettingKind::Choice(words) => format!("one of {}", words.join(", ")),
            SettingKind::Flag => "true or false".to_string(),
            SettingKind::Text => "text".to_string(),
            SettingKind::Json => "JSON".to_string(),
        }
    }

    /// A value checked against the setting's nature and bounds, normalised (a whole number given as `3.0` is `3`).
    pub fn check(&self, value: &Value) -> Result<Value, String> {
        match &self.kind {
            SettingKind::Number { least, most, .. } => {
                let Some(number) = value.as_f64().filter(|n| n.is_finite()) else {
                    return Err(format!("`{value}` is not a number"));
                };
                if least.is_some_and(|least| number < least) || most.is_some_and(|most| number > most) {
                    return Err(format!("{number} is not {}", self.nature()));
                }
                Ok(Value::from(number))
            }
            SettingKind::Integer { least, most } => {
                let whole = value.as_i64().or_else(|| {
                    value
                        .as_f64()
                        .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)
                        .map(|n| n as i64)
                });
                let Some(whole) = whole else {
                    return Err(format!("`{value}` is not a whole number"));
                };
                if least.is_some_and(|least| whole < least) || most.is_some_and(|most| whole > most) {
                    return Err(format!("{whole} is not {}", self.nature()));
                }
                Ok(Value::from(whole))
            }
            SettingKind::Choice(words) => match value.as_str() {
                Some(word) if words.contains(&word) => Ok(Value::from(word)),
                _ => Err(format!("`{value}` is not {}", self.nature())),
            },
            SettingKind::Flag => value
                .as_bool()
                .map(Value::Bool)
                .ok_or_else(|| format!("`{value}` is not true or false")),
            SettingKind::Text => value
                .as_str()
                .map(Value::from)
                .ok_or_else(|| format!("`{value}` is not text")),
            SettingKind::Json => Ok(value.clone()),
        }
    }

    /// A value from its text, as a command line gives it, read by the setting's nature: a number, a word, `true`,
    /// or JSON for a [`SettingKind::Json`] setting.
    pub fn parse(&self, text: &str) -> Result<Value, String> {
        let text = text.trim();
        let value = match &self.kind {
            SettingKind::Number { .. } | SettingKind::Integer { .. } => {
                let number: f64 = text.parse().map_err(|_| format!("`{text}` is not a number"))?;
                Value::from(number)
            }
            SettingKind::Flag => match text {
                "true" | "yes" | "1" => Value::Bool(true),
                "false" | "no" | "0" => Value::Bool(false),
                _ => return Err(format!("`{text}` is not true or false")),
            },
            SettingKind::Choice(_) | SettingKind::Text => Value::from(text),
            SettingKind::Json => {
                serde_json::from_str(text).map_err(|why| format!("`{text}` is not JSON: {why}"))?
            }
        };
        self.check(&value)
    }
}

/// The settings of one request: only declared names, each checked, the defaults filled in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settled(BTreeMap<String, Value>);

impl Settled {
    /// The settings given checked against the declared ones, the defaults added.
    ///
    /// # Errors
    ///
    /// A name the view does not declare, a value that does not fit, a required setting not given.
    pub fn settle(
        view: &str,
        declared: &[ViewSetting],
        given: &BTreeMap<String, Value>,
    ) -> Result<Settled, ViewRefusal> {
        let mut out = BTreeMap::new();
        for (name, value) in given {
            let Some(setting) = declared.iter().find(|setting| setting.name == name) else {
                let known: Vec<&str> = declared.iter().map(|setting| setting.name).collect();
                return Err(ViewRefusal::UnknownSetting {
                    view: view.to_string(),
                    name: name.clone(),
                    known: if known.is_empty() {
                        "none".to_string()
                    } else {
                        known.join(", ")
                    },
                });
            };
            let checked = setting.check(value).map_err(|why| ViewRefusal::BadSetting {
                name: name.clone(),
                why,
            })?;
            out.insert(name.clone(), checked);
        }
        for setting in declared {
            if out.contains_key(setting.name) {
                continue;
            }
            match &setting.default {
                Some(default) => {
                    out.insert(setting.name.to_string(), default.clone());
                }
                None if setting.required => {
                    return Err(ViewRefusal::MissingSetting {
                        view: view.to_string(),
                        name: setting.name.to_string(),
                        about: setting.about.to_string(),
                    });
                }
                None => {}
            }
        }
        Ok(Settled(out))
    }

    /// The value of a setting, when it has one.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// A number setting.
    pub fn number(&self, name: &str) -> Option<f64> {
        self.0.get(name).and_then(Value::as_f64)
    }

    /// A whole number setting.
    pub fn integer(&self, name: &str) -> Option<i64> {
        self.0.get(name).and_then(Value::as_i64)
    }

    /// A word or text setting.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(Value::as_str)
    }

    /// A flag setting; `false` when absent.
    pub fn flag(&self, name: &str) -> bool {
        self.0.get(name).and_then(Value::as_bool).unwrap_or(false)
    }

    /// Every setting, by name.
    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.0
    }
}
