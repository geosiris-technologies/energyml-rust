//! An operation: a pure function from a bag of inputs to a plan of objects.
//!
//! In one sentence: **a creation is a pure
//! function from [`Pieces`] to a [`Plan`], and applying the plan is the only
//! step that touches storage.** This module holds the trait and the three
//! values around it — what an operation still needs to be told ([`Slot`]),
//! what it decided ([`Plan`]), and how the objects it makes are named
//! ([`Identity`]) — plus the registry that answers *what can I create with
//! this?*
//!
//! # Purity is held by the signature
//!
//! [`AnyOperation::plan`] takes no store. It cannot read one, so it cannot depend
//! on one, and a test reaches it with values alone. What the caller knows of
//! the package arrives resolved, as [`crate::pieces::Known`].
//!
//! # A refusal is named, never a boolean
//!
//! [`AnyOperation::fitness`] returns *why not* — *a set of polylines is not
//! written as a triangulated surface* — for the reason `Exporter::handles`
//! already gives: somebody who wrote a rule deserves to know why it does not
//! apply. And [`Registry::offers`] lists the unfit operations **with their
//! reason** rather than dropping them, because *absent* reads as *not written
//! yet*.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;
use uuid::Uuid;

use crate::pieces::{Pieces, Ref};

/// What a slot asks for — rendered as a command-line option, an interactive
/// question or a form field, so that a new operation writes a line in neither.
#[derive(Debug, Clone, PartialEq)]
pub enum Need {
    /// An object of the package, of one of these qualified types (empty: any).
    Object {
        /// `resqml22.TriangulatedSetRepresentation`, …
        types: &'static [&'static str],
    },
    /// A property kind — a well-known one or one of the package.
    PropertyKind,
    /// A local CRS of the package.
    Crs,
    /// An interpretation of the package.
    Interpretation,
    /// A measured number, possibly bounded.
    Number {
        /// The unit the number is read in, when it has one.
        uom: Option<&'static str>,
        /// Inclusive lower bound.
        least: Option<f64>,
        /// Inclusive upper bound.
        most: Option<f64>,
    },
    /// A whole number, possibly bounded.
    Integer {
        /// Inclusive lower bound.
        least: Option<i64>,
        /// Inclusive upper bound.
        most: Option<i64>,
    },
    /// Free text.
    Text,
    /// Yes or no.
    Flag,
    /// One of a closed list.
    Choice(&'static [&'static str]),
    /// An EPSG code.
    Epsg,
    /// A **type** the build knows, picked from a filtered list.
    ///
    /// The type list shows the objects that extend `AbstractObject`, with a
    /// default filter that the user can change, and the operation says whether the
    /// package and the version are its own defaults or a rule — which is what
    /// decides whether the front end greys the control.
    ///
    /// The **list** is not here: it comes from the schema registry, which the
    /// caller owns. [`crate::types`] holds the sorting, which can be wrong.
    Type(crate::types::TypeChoice),
    /// A title for the objects made — one per plan, the rest derived by role.
    Title,
    /// **A term of a named vocabulary**, picked from its tree — `geolts`, the
    /// BRGM chronostratigraphic chart. The answer is text a person can
    /// type (a label, a code, an id); the **tree** is the caller's to draw,
    /// asked of the library by the vocabulary's name, so the front end renders it
    /// without knowing what the chart is. What a term names is checked by the
    /// operation, which holds the vocabulary — here, only the shape.
    Vocabulary(&'static str),
    /// **An input of the scene**: what the operation works on.
    ///
    /// This is the declaration that was missing. An operation said what it
    /// **asks** and what it **reports**, and it **judged** a bag — but it did not say
    /// what it *takes*, and `fitness` is a verdict given after the fact on a bag
    /// already made: it cannot be used to ask. Measured consequence:
    /// `crs`, the only operation fit for an empty bag, was unreachable
    /// from a front end that demanded an input before offering anything.
    ///
    /// The answer is a **name of a piece of the bag**, not a uuid: an input of the
    /// scene may be an object of the package or an artefact, and only the caller
    /// knows what the scene holds.
    Input(Takes),
}

/// What an input slot takes.
///
/// **The vocabulary is that of [`crate::pieces::Source::kind`]**, and not a
/// second one: `shape`, `features`, `lattice`, `field`, `ramp`. Two ways of
/// naming the same thing would end up no longer naming the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Takes {
    /// The kind of piece — `shape`, `features`, `lattice`, `field`, `ramp`.
    pub kind: &'static str,
    /// The expected topology of a shape, when there is one.
    ///
    /// **A filter, not a rule**: it sorts the list the form
    /// proposes, and the refusal stays with `fitness`, which alone sees the whole bag. The
    /// same division as `Need::Type`, where the filter is proposed and the constraint
    /// stated separately.
    pub topology: Option<&'static str>,
    /// The qualified types that an **object** slot ranks first.
    ///
    /// **A filter, not a rule**, exactly like `topology`: it tells a
    /// form what to propose first, and the refusal stays with the operation, which
    /// alone sees what the object really carries. Empty means *any* —
    /// the front end's selector rule, applied here.
    ///
    /// Added with `ObjectIn`: without it, a slot that expects a
    /// `ColumnBasedTable` would list the two thousand objects of the package, which
    /// is a list in which nothing can be found.
    pub types: &'static [&'static str],
    /// **A ramp read as a palette**: sampled over its own range, its levels
    /// saying *where along the scale* and nothing about the values — so a form
    /// may offer its palettes for it (`isocontours`' colours).
    ///
    /// Declared, never deduced from the kind: `colormap` takes a ramp too, and
    /// there the levels **are** the values. A palette handed to it would write a
    /// map over 0..1 that looks like a colour map and draws nothing right.
    pub palette: bool,
}

impl Takes {
    /// A shape, with no expected topology.
    pub const fn shape() -> Self {
        Self {
            kind: "shape",
            topology: None,
            types: &[],
            palette: false,
        }
    }

    /// A shape whose topology is expected — `triangles`, `polylines`, `points`.
    pub const fn shaped(topology: &'static str) -> Self {
        Self {
            kind: "shape",
            topology: Some(topology),
            types: &[],
            palette: false,
        }
    }

    /// A piece of another kind: `features`, `lattice`, `field`, `ramp`.
    pub const fn piece(kind: &'static str) -> Self {
        Self {
            kind,
            topology: None,
            types: &[],
            palette: false,
        }
    }

    /// An object of the package, of which these types are proposed.
    pub const fn object(types: &'static [&'static str]) -> Self {
        Self {
            kind: "object",
            topology: None,
            types,
            palette: false,
        }
    }

    /// Whether an object of this qualified type is one this slot proposes —
    /// `true` for any when it names none.
    ///
    /// **On the bare class name**, ignoring case, the package prefix and the
    /// `obj_` that RESQML 2.0.1 spells: the same rule as `accepts` in the
    /// front end, so that the list a form offers and what [`crate::read::every`]
    /// takes are the same objects.
    pub fn accepts(&self, qualified_type: &str) -> bool {
        fn bare(qualified: &str) -> String {
            let class = qualified
                .rsplit('.')
                .next()
                .unwrap_or(qualified)
                .to_ascii_lowercase();
            class.strip_prefix("obj_").map(str::to_string).unwrap_or(class)
        }
        self.types.is_empty() || self.types.iter().any(|one| bare(one) == bare(qualified_type))
    }
}

impl Need {
    /// The EML `ActivityParameterKind` this becomes in a template.
    pub fn parameter_kind(&self) -> &'static str {
        match self {
            Need::Object { .. } | Need::PropertyKind | Need::Crs | Need::Interpretation => "dataObject",
            // A type is a name, not an object of the package: nothing points at
            // it, and writing it as a `dataObject` would make the activity
            // reference a part that does not exist.
            Need::Type(_) => "string",
            Need::Number { .. } | Need::Epsg => "double",
            Need::Integer { .. } => "integer",
            // EML 2.3 has no boolean kind — `dataObject`, `double`, `integer`,
            // `string`, `timestamp`, `subActivity` — so a flag is a string, and
            // the template's `Constraint` says which two.
            Need::Text | Need::Choice(_) | Need::Title | Need::Flag | Need::Vocabulary(_) => "string",
            // An input **is not a template parameter**: the activity
            // already records its inputs as such, and declaring them twice
            // would make the same thing an input and a setting. The
            // projection does not see them — `takes()` is separate from `needs()`.
            Need::Input(_) => "dataObject",
        }
    }

    /// Whether this value has the shape the need asks for.
    pub fn accepts(&self, value: &Value) -> Result<(), String> {
        let bad = |what: &str| Err(format!("expected {what}, got {value}"));
        match self {
            Need::Object { .. } | Need::PropertyKind | Need::Crs | Need::Interpretation => {
                if value.is_string() || value.is_object() {
                    Ok(())
                } else {
                    bad("an object reference (a uuid, or a DataObjectReference)")
                }
            }
            Need::Number { least, most, .. } => match value.as_f64() {
                Some(v) if least.is_none_or(|l| v >= l) && most.is_none_or(|m| v <= m) => Ok(()),
                Some(v) => Err(format!("{v} is outside [{}, {}]", bound(*least), bound(*most))),
                None => bad("a number"),
            },
            Need::Epsg => match value.as_i64() {
                Some(v) if v > 0 => Ok(()),
                _ => bad("an EPSG code (a positive whole number)"),
            },
            Need::Integer { least, most } => match value.as_i64() {
                Some(v) if least.is_none_or(|l| v >= l) && most.is_none_or(|m| v <= m) => Ok(()),
                Some(v) => Err(format!("{v} is outside [{}, {}]", bound(*least), bound(*most))),
                None => bad("a whole number"),
            },
            Need::Text | Need::Title => {
                if value.is_string() {
                    Ok(())
                } else {
                    bad("text")
                }
            }
            Need::Vocabulary(name) => match value.as_str() {
                Some(text) if !text.trim().is_empty() => Ok(()),
                _ => bad(&format!("a term of `{name}` — its label, its code or its id")),
            },
            // The **name of a piece of the bag**. What it designates is the
            // caller's business, since only it holds the scene; here the shape is checked.
            Need::Input(_) => match value.as_str() {
                Some(text) if !text.is_empty() => Ok(()),
                _ => bad("the name of an input"),
            },
            // Its shape only — that it names a type this build knows is the
            // caller's to check, because only the caller has the registry.
            Need::Type(_) => match value.as_str() {
                Some(text) if text.contains('.') && !text.starts_with('.') && !text.ends_with('.') => Ok(()),
                Some(_) => {
                    bad("a qualified type, `package` and `Type` around a dot (resqml22.BoundaryFeature)")
                }
                None => bad("a qualified type"),
            },
            Need::Flag => {
                if value.is_boolean() {
                    Ok(())
                } else {
                    bad("true or false")
                }
            }
            Need::Choice(allowed) => match value.as_str() {
                Some(s) if allowed.contains(&s) => Ok(()),
                _ => bad(&format!("one of {}", allowed.join(", "))),
            },
        }
    }
}

impl Need {
    /// The need, for a form that does not know Rust: its kind, and what
    /// narrows it.
    pub fn json(&self) -> Value {
        match self {
            Need::Object { types } => serde_json::json!({ "kind": "object", "types": types }),
            Need::PropertyKind => serde_json::json!({ "kind": "propertyKind" }),
            // The systems a geometry may name, **whole** — one list with what
            // `support::frame_in` accepts, so the form offers what the
            // operation takes (`crate::crs::FRAMES`).
            Need::Crs => serde_json::json!({ "kind": "crs", "types": crate::crs::FRAMES }),
            Need::Interpretation => serde_json::json!({ "kind": "interpretation" }),
            Need::Number { uom, least, most } => {
                serde_json::json!({ "kind": "number", "uom": uom, "least": least, "most": most })
            }
            Need::Integer { least, most } => {
                serde_json::json!({ "kind": "integer", "least": least, "most": most })
            }
            Need::Text => serde_json::json!({ "kind": "text" }),
            Need::Flag => serde_json::json!({ "kind": "flag" }),
            Need::Choice(allowed) => serde_json::json!({ "kind": "choice", "choices": allowed }),
            Need::Epsg => serde_json::json!({ "kind": "epsg" }),
            Need::Type(choice) => {
                let mut out = choice.json();
                out["kind"] = serde_json::json!("type");
                out
            }
            Need::Title => serde_json::json!({ "kind": "title" }),
            Need::Vocabulary(name) => serde_json::json!({ "kind": "vocabulary", "vocabulary": name }),
            // What the form reads to sort its list: the kind of
            // piece, and the expected topology when there is one.
            Need::Input(takes) => {
                serde_json::json!({
                    "kind": "input",
                    "takes": takes.kind,
                    "topology": takes.topology,
                    "types": takes.types,
                    "palette": takes.palette,
                })
            }
        }
    }
}

fn bound<T: fmt::Display>(bound: Option<T>) -> String {
    bound.map(|b| b.to_string()).unwrap_or_else(|| "∞".to_string())
}

/// When a slot can act at all.
///
/// # Why this is not `needs(pieces, choices)`
///
/// Two slots of the same operation are often **alternatives**: `crs` names a
/// system of the package, `sourceEpsg` builds one — answering either makes the
/// other meaningless, and a form that shows both says nothing about it. The
/// obvious fix, letting `needs` see the answers, breaks the other reader:
/// [`AnyOperation::needs`] must answer on the **empty** bag because that is what
/// `rc/activityTemplates.json` is projected from, and a slot that vanished with
/// an answer would vanish from the template too.
///
/// So it is declared, not computed. The template keeps every slot; a form hides
/// what cannot act (a setting that cannot act is not shown); and [`Choices::settle`] neither requires nor fills an
/// inapplicable slot.
///
/// # The two halves read two different things, and the asymmetry is the point
///
/// `With` asks *will that slot have a value?* — answered **or** suggested. A
/// caller who sets `project` alone and lets `sourceEpsg` take its suggested
/// 4326 is doing something perfectly ordinary, and a test written before this
/// existed says so.
///
/// `Without` asks *did somebody decide the other way?* — answered **only**. A
/// suggestion is not a decision, and letting one veto its alternative would
/// hide the other half of the frame before anybody had chosen. It would also be
/// circular: settling fills suggestions, which would change what applies, which
/// would change what is filled.
///
/// A form is stricter, and legitimately so: everything it holds in a field is a
/// value, a pre-filled field included, so picking a CRS hides the EPSG codes
/// at once — which is exactly what was asked for. The two agree in practice
/// because a form sends what it holds.
///
/// # The third case: *only for that value of that slot*
///
/// `When` is what a **method** needs: `kernel` means something to `rbf` and nothing to `idw`. Before it,
/// the only way to say so was the help text, and it cost twice — a form showed
/// every sub-parameter of every method, and **an IDW run recorded `kernel:
/// thin-plate`** in its activity, because a slot with a suggestion that always
/// applies is always settled, and whatever is settled is recorded.
///
/// It reads the controlling slot's **settled** value — answered, else
/// suggested — and it reads it **after** that slot was itself settled, in
/// declaration order. That is what makes the conditions nest: a sub-slot of a
/// method that does not apply finds its controller removed, and goes with it.
/// So a controller must be declared before what it controls, and
/// [`Choices::settle_for`] refuses a declaration that does otherwise.
///
/// # The fourth: *when one of those holds*
///
/// `Any` is the pivot of `transform`: a rotation turns about it and a scale is
/// taken from it, and a translation alone never reads it. Without `Any` the
/// pivot would always apply, always be settled — so a pure translation would
/// record `pivot: centre` in its activity, the defect `When` was made for, by
/// another road. Each branch is a `When`; its controllers, like any `When`'s,
/// are declared before the slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applies {
    /// Always.
    Always,
    /// Only when every one of those slots will have a value.
    With(&'static [&'static str]),
    /// Only when none of those slots was answered.
    Without(&'static [&'static str]),
    /// Only when that slot's settled value is one of those.
    When {
        /// The controlling slot — declared before this one.
        slot: &'static str,
        /// The values under which this slot acts.
        is: Vec<&'static str>,
    },
    /// Only when one of those conditions holds.
    Any(Vec<Applies>),
}

impl Applies {
    /// Whether the slot applies — see the notes on the predicates above.
    ///
    /// `answered` is the caller's own answer, `effective` whether a slot will
    /// have a value (answered or suggested), `settled` the value a slot has
    /// **once settled** — `None` when it was removed or left empty.
    pub fn now(
        &self,
        answered: impl Fn(&str) -> bool,
        effective: impl Fn(&str) -> bool,
        settled: impl Fn(&str) -> Option<Value>,
    ) -> bool {
        self.holds(&answered, &effective, &settled)
    }

    /// [`Applies::now`], through references — `Any` recurses, and a generic
    /// recursion would instantiate itself without end.
    fn holds(
        &self,
        answered: &dyn Fn(&str) -> bool,
        effective: &dyn Fn(&str) -> bool,
        settled: &dyn Fn(&str) -> Option<Value>,
    ) -> bool {
        match self {
            Applies::Always => true,
            Applies::With(names) => names.iter().all(|name| effective(name)),
            Applies::Without(names) => !names.iter().any(|name| answered(name)),
            Applies::When { slot, is } => settled(slot).is_some_and(|value| match &value {
                Value::String(text) => is.contains(&text.as_str()),
                // A flag's value is a boolean on the wire and a word in the
                // declaration (`when = "bands: true"`): the two are one fact.
                Value::Bool(flag) => is.contains(&if *flag { "true" } else { "false" }),
                _ => false,
            }),
            Applies::Any(all) => all.iter().any(|one| one.holds(answered, effective, settled)),
        }
    }

    /// The names this depends on, for a form that must re-ask when they move.
    pub fn names(&self) -> Vec<&'static str> {
        match self {
            Applies::Always => Vec::new(),
            Applies::With(names) | Applies::Without(names) => names.to_vec(),
            Applies::When { slot, .. } => vec![slot],
            Applies::Any(all) => {
                let mut out: Vec<&'static str> = Vec::new();
                for name in all.iter().flat_map(Applies::names) {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
                out
            }
        }
    }

    /// The slots whose **settled value** this reads — each must be declared
    /// before the slot it conditions.
    pub fn controllers(&self) -> Vec<&'static str> {
        match self {
            Applies::When { slot, .. } => vec![slot],
            Applies::Any(all) => all.iter().flat_map(Applies::controllers).collect(),
            _ => Vec::new(),
        }
    }

    /// The condition in words — `` `rotate` is `axis`, or `scale` is `uniform` or `axes` ``.
    pub fn said(&self) -> String {
        match self {
            Applies::Always => "always".to_string(),
            Applies::With(names) => format!("`{}` has a value", names.join("`, `")),
            Applies::Without(names) => format!("`{}` was not given", names.join("`, `")),
            Applies::When { slot, is } => {
                format!(
                    "`{slot}` is {}",
                    is.iter()
                        .map(|one| format!("`{one}`"))
                        .collect::<Vec<_>>()
                        .join(" or ")
                )
            }
            Applies::Any(all) => all.iter().map(Applies::said).collect::<Vec<_>>().join(", or "),
        }
    }

    /// On the wire, for a form — the front end reads it.
    pub fn json(&self) -> Value {
        match self {
            Applies::Always => serde_json::json!({ "kind": "always" }),
            Applies::With(names) => serde_json::json!({ "kind": "with", "names": names }),
            Applies::Without(names) => serde_json::json!({ "kind": "without", "names": names }),
            Applies::When { slot, is } => serde_json::json!({ "kind": "when", "slot": slot, "is": is }),
            Applies::Any(all) => {
                serde_json::json!({ "kind": "any", "of": all.iter().map(Applies::json).collect::<Vec<_>>() })
            }
        }
    }

    /// Whether this slot acts only under a condition — what makes it
    /// `MinOccurs 0` in a template, whatever it requires when it does act.
    pub fn is_conditional(&self) -> bool {
        !matches!(self, Applies::Always)
    }
}

/// What leaving a slot empty produces.
///
/// The form asks for the interpretation; could it also offer to create it,
/// with its feature, and link them correctly? The operation
/// already did — leaving `interpretation` out of `well-from-polyline` writes the
/// feature, the interpretation and the links. **Nobody could know**: the slot
/// was merely *optional*, and an optional field reads *you may do without*, not
/// *I make it for you*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Otherwise {
    /// The field is simply absent from what is written.
    Nothing,
    /// The suggestion applies — what `suggested` already says.
    Default,
    /// Those objects are created here, and linked.
    Creates {
        /// The qualified types, in the order they are written. A `Vec` and not
        /// a `&'static` slice so that an operation can name **its** pair
        /// without a table of pairs living somewhere else; two strings per slot
        /// is not a cost worth a lookup.
        types: Vec<&'static str>,
    },
    /// An **input** slot left empty takes every object of the target package
    /// whose type is one of the slot's `types` — see [`crate::read::every`].
    ///
    /// The use case, about the wells of `wells-from-table`: there is no reason
    /// to select the wells when they are in the current storage. All the wells
    /// of the current stage should be considered used by default to
    /// enrich them.
    /// Declared, not guessed by a face: the front end and the CLI fill
    /// the slot through the same function, and an operation that does not say
    /// it keeps an empty slot empty.
    Every,
}

/// Where a proposed value came from — one rendering, four sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// Read off one of the inputs — the CRS of the contoured representation.
    Input(Ref),
    /// Copied from an object the person pointed at as a model.
    Exemplar(Ref),
    /// Taken from the parameters a past run recorded.
    Activity(Ref),
    /// The operation's own default, which belongs to nobody.
    Default,
}

impl Provenance {
    /// The object it came from, when it came from one.
    pub fn reference(&self) -> Option<&Ref> {
        match self {
            Provenance::Input(one) | Provenance::Exemplar(one) | Provenance::Activity(one) => Some(one),
            Provenance::Default => None,
        }
    }

    /// A stable word for the front end to translate.
    pub fn kind(&self) -> &'static str {
        match self {
            Provenance::Input(_) => "input",
            Provenance::Exemplar(_) => "exemplar",
            Provenance::Activity(_) => "activity",
            Provenance::Default => "default",
        }
    }

    /// The provenance, for a form: its kind and the object it names.
    pub fn json(&self) -> Value {
        serde_json::json!({ "kind": self.kind(), "from": self.reference().map(|r| r.dor()) })
    }
}

/// What an operation still needs to be told — and what it already proposes.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// The parameter's name — also the `ParameterTemplate` title.
    pub name: &'static str,
    /// What kind of answer.
    pub need: Need,
    /// One line for `--list` and for a form's label.
    pub about: &'static str,
    /// How many values at least (`MinOccurs`).
    pub least: u32,
    /// How many at most (`MaxOccurs`); `None` is unbounded.
    pub most: Option<u32>,
    /// Whether the **order** of those values is part of the answer.
    ///
    /// Only meaningful on a slot that takes several (`Slot::takes_many`), and
    /// it decides how a form renders: a list somebody reorders by hand, or a
    /// set of boxes somebody ticks.
    ///
    /// It is declared rather than guessed because of what a front end's ordering code says of
    /// the one case that already exists — the surfaces of a section, top to
    /// bottom:
    ///
    /// > *This is the most dangerous datum of the whole section. Get it the
    /// > wrong way round, and an inverted section looks abnormal to nobody:
    /// > same colours, same thicknesses, only the geology is wrong.*
    ///
    /// A front end that inferred it would be deciding that, and the module that
    /// computes the order refuses to decide it.
    pub ordered: bool,
    /// A value **inferred from the inputs**, modifiable: the CRS of the
    /// representation being contoured, the highest Z of a polyline for a well's
    /// ground. Pre-filled, editable.
    pub suggested: Option<Value>,
    /// Where the suggestion came from, when there is one.
    pub from: Option<Provenance>,
    /// When this slot can act at all — see [`Applies`].
    pub applies: Applies,
    /// What leaving it empty produces — see [`Otherwise`].
    pub otherwise: Otherwise,
    /// Which block of a form it belongs in: `frame`, `represents`, `operation`,
    /// `naming`. A dozen fields laid flat is a common complaint; four short
    /// blocks is the same dozen, read.
    pub group: &'static str,
    /// Where this value lives on an object of the same sort, in reflective
    /// field paths tried in order — what makes *copy it from that other
    /// wellbore* possible without a line of code per operation.
    ///
    /// Empty means **not copyable**, and a form must then not offer it: a
    /// control that cannot act is not shown.
    pub reads: &'static [&'static str],
    /// A setting most runs leave alone — folded away in a form, like QGIS's
    /// `FlagAdvanced`. Seventeen fields for `interpolate` is the complaint;
    /// `epsilon` and `nugget` are what nobody sets first.
    pub advanced: bool,
}

impl Slot {
    /// A slot that must be filled.
    pub fn required(name: &'static str, need: Need, about: &'static str) -> Self {
        Self {
            name,
            need,
            about,
            least: 1,
            most: Some(1),
            ordered: false,
            suggested: None,
            from: None,
            applies: Applies::Always,
            otherwise: Otherwise::Nothing,
            group: "operation",
            reads: &[],
            advanced: false,
        }
    }

    /// A slot that may be left empty.
    pub fn optional(name: &'static str, need: Need, about: &'static str) -> Self {
        Self {
            least: 0,
            ..Self::required(name, need, about)
        }
    }

    /// A slot that takes **several** values, `least` at fewest and no ceiling.
    ///
    /// The answer is then a JSON **array**, always — see [`Slot::takes_many`].
    /// Its order is **not** data unless [`Slot::in_order`] says so.
    pub fn many(name: &'static str, need: Need, about: &'static str, least: u32) -> Self {
        Self {
            least,
            most: None,
            ..Self::required(name, need, about)
        }
    }

    /// The same slot, its values' **order** being part of the answer.
    ///
    /// Refused on a slot that takes a single value: there is no order of one,
    /// and letting it be set would put a flag in the template that a form would
    /// then have to decide how to ignore.
    pub fn in_order(mut self) -> Self {
        debug_assert!(
            self.takes_many(),
            "`{}` takes one value: it has no order",
            self.name
        );
        self.ordered = self.takes_many();
        self
    }

    /// The same slot, with a ceiling on how many values it takes.
    pub fn at_most(mut self, most: u32) -> Self {
        self.most = Some(most);
        self
    }

    /// Whether this slot's answer is a list rather than a single value.
    ///
    /// **Exactly one** value is the single-value case; everything else is a
    /// list, including `0..1` — because *may be absent* and *may hold one* are
    /// the same shape, and `Slot::optional` has always meant the first.
    ///
    /// The rule has to be this crisp because it decides a *wire format*: the
    /// front end writes what this says, and a slot whose shape depended on how many
    /// values happened to be chosen would be read one way and written another.
    pub fn takes_many(&self) -> bool {
        self.most != Some(1)
    }

    /// The same slot, with a value proposed.
    pub fn suggest(mut self, value: Value) -> Self {
        self.suggested = Some(value);
        self
    }

    /// The same slot, saying which input the proposal was read from.
    pub fn inferred(mut self, from: Ref) -> Self {
        self.from = Some(Provenance::Input(from));
        self
    }

    /// The same slot, with its provenance said in full.
    pub fn came_from(mut self, from: Provenance) -> Self {
        self.from = Some(from);
        self
    }

    /// The same slot, applying only under that condition.
    pub fn when(mut self, applies: Applies) -> Self {
        self.applies = applies;
        self
    }

    /// The same slot, saying what its emptiness produces.
    pub fn otherwise(mut self, otherwise: Otherwise) -> Self {
        self.otherwise = otherwise;
        self
    }

    /// The same slot, in that block of a form.
    pub fn grouped(mut self, group: &'static str) -> Self {
        self.group = group;
        self
    }

    /// The same slot, readable off an object of the same sort at those paths.
    pub fn reads(mut self, paths: &'static [&'static str]) -> Self {
        self.reads = paths;
        self
    }

    /// The same slot, acting only when `slot` settles to one of `is`.
    pub fn only_when(mut self, slot: &'static str, is: &[&'static str]) -> Self {
        self.applies = Applies::When {
            slot,
            is: is.to_vec(),
        };
        self
    }

    /// The same slot, folded away in a form.
    pub fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }

    /// Whether it can act — see [`Applies::now`] for the three readings.
    pub fn applies_now(
        &self,
        answered: impl Fn(&str) -> bool,
        effective: impl Fn(&str) -> bool,
        settled: impl Fn(&str) -> Option<Value>,
    ) -> bool {
        self.applies.now(answered, effective, settled)
    }

    /// Filled, or has a suggestion to fall back on.
    pub fn is_required(&self) -> bool {
        self.least > 0
    }

    /// The slot as a form field: name, need, whether required, the suggestion
    /// and where it came from.
    pub fn json(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "need": self.need.json(),
            "about": self.about,
            "required": self.is_required(),
            "least": self.least,
            "most": self.most,
            // Said rather than rederived on the front end side. The rule — *anything
            // that is not exactly one is a list* — decides a wire format, and
            // two readers of a wire format is exactly what this repository
            // pays for every time it lets them exist.
            "takesMany": self.takes_many(),
            "ordered": self.ordered,
            "suggested": self.suggested,
            "inferredFrom": self.from.as_ref().and_then(Provenance::reference).map(|r| r.dor()),
            "from": self.from.as_ref().map(Provenance::json),
            "applies": self.applies.json(),
            "otherwise": match &self.otherwise {
                Otherwise::Nothing => serde_json::json!({ "kind": "nothing" }),
                Otherwise::Default => serde_json::json!({ "kind": "default" }),
                Otherwise::Creates { types } => serde_json::json!({ "kind": "creates", "types": types }),
                Otherwise::Every => serde_json::json!({ "kind": "every" }),
            },
            "group": self.group,
            "reads": self.reads,
            "advanced": self.advanced,
        })
    }
}

/// The answers given to the slots, by name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Choices(BTreeMap<String, Value>);

impl Choices {
    /// No answer yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The choices with one more answer.
    pub fn set(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.0.insert(name.to_string(), value.into());
        self
    }

    /// The raw answer.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// A text answer.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(Value::as_str)
    }

    /// A number.
    pub fn number(&self, name: &str) -> Option<f64> {
        self.get(name).and_then(Value::as_f64)
    }

    /// A whole number.
    pub fn integer(&self, name: &str) -> Option<i64> {
        self.get(name).and_then(Value::as_i64)
    }

    /// A flag.
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(Value::as_bool)
    }

    /// Every answer, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Whether the caller answered that slot — the test [`Applies`] is read
    /// against, and it is the caller's own answers, never the suggestions.
    fn answered(&self, name: &str) -> bool {
        self.0.get(name).is_some_and(|value| !value.is_null())
    }

    /// The answers, **completed with the suggestions** for what was left out,
    /// and checked: a required slot with neither answer nor suggestion, or an
    /// answer of the wrong shape, is refused by name **before any work**.
    ///
    /// **A slot that cannot act is neither required nor filled**, and one that
    /// was answered anyway is refused *naming both ends* — `project` given
    /// beside a `crs` of the package is a contradiction, and dropping it in
    /// silence would reproject into a system nobody asked for. A form hides
    /// those fields, so only a caller writing choices by hand can reach this.
    pub fn settle(&self, slots: &[Slot]) -> Result<Choices, Error> {
        self.settle_for(slots, Purpose::Writing)
    }

    /// The same, for a plan asked for its **geometry only**.
    ///
    /// A slot that only the writing needs — everything outside the
    /// `operation` group: the frame, what is represented, the naming — may
    /// then stay empty: its suggestion is still applied when it has one, and
    /// its absence is not a refusal. What the *computation* needs is required
    /// as before, because a tolerance nobody gave is not a geometry anybody
    /// asked for.
    pub fn settle_for(&self, slots: &[Slot], purpose: Purpose) -> Result<Choices, Error> {
        self.settle_given(slots, purpose, &[])
    }

    /// The same, knowing which **input** slots the bag fills —
    /// [`crate::declare::given_inputs`], which [`Registry::plan_in`] passes.
    ///
    /// A condition may name an input: `spacing` of `interpolate` acts only
    /// `Without(["onto"])`, the grid whose lattice replaces it. An input is
    /// never among the answers, so without this the condition could not be
    /// met, and a run given `onto` would still be asked its `spacing`. Given
    /// counts as answered for [`Applies`] **only**: a given input is not a
    /// choice, so it is neither checked nor recorded as one.
    pub fn settle_given(&self, slots: &[Slot], purpose: Purpose, given: &[&str]) -> Result<Choices, Error> {
        let mut out = self.clone();
        let mut wrong: Vec<Refusal> = Vec::new();
        let answered = |name: &str| self.answered(name) || given.contains(&name);
        // *Will that slot have a value?* — answered, or holding a suggestion
        // that settling is about to apply.
        let effective = |name: &str| {
            answered(name)
                || slots
                    .iter()
                    .any(|slot| slot.name == name && slot.suggested.is_some())
        };
        // A `When` reads its controller **as settled**, so the controller must
        // come first — otherwise the answer would depend on an order nobody
        // can see. Refused as a declaration fault, not a caller's.
        for (at, slot) in slots.iter().enumerate() {
            for controller in slot.applies.controllers() {
                if !slots[..at].iter().any(|one| one.name == controller) {
                    return Err(Refusal::about(
                        "declaration",
                        slot.name,
                        format!(
                            "`{}` acts on the value of `{controller}`, which is not declared before it",
                            slot.name
                        ),
                    )
                    .into());
                }
            }
        }
        for slot in slots {
            let applies = {
                let settled = |name: &str| out.0.get(name).filter(|value| !value.is_null()).cloned();
                slot.applies_now(answered, effective, settled)
            };
            if !applies {
                if self.answered(slot.name) {
                    let against = slot.applies.names().join("`, `");
                    let message = match &slot.applies {
                        Applies::With(_) => format!("`{}` needs `{against}`, which has no value", slot.name),
                        Applies::When { .. } | Applies::Any(_) => {
                            format!(
                                "`{}` acts only when {} — it is not here",
                                slot.name,
                                slot.applies.said()
                            )
                        }
                        _ => format!(
                            "`{}` cannot be given beside `{against}`: they are alternatives",
                            slot.name
                        ),
                    };
                    wrong.push(Refusal::about("inapplicable", slot.name, message));
                }
                out.0.remove(slot.name);
                continue;
            }
            match out.0.get(slot.name) {
                Some(value) if slot.takes_many() => wrong.extend(check_many(slot, value)),
                Some(value) => {
                    if let Err(why) = slot.need.accepts(value) {
                        wrong.push(Refusal::about(
                            "value",
                            slot.name,
                            format!("`{}`: {why}", slot.name),
                        ));
                    }
                }
                None => match &slot.suggested {
                    Some(value) => {
                        out.0.insert(slot.name.to_string(), value.clone());
                    }
                    None if slot.is_required()
                        && (purpose == Purpose::Writing || slot.group == "operation") =>
                    {
                        wrong.push(Refusal::about(
                            "required",
                            slot.name,
                            format!("`{}` is required: {}", slot.name, slot.about),
                        ));
                    }
                    None => {}
                },
            }
        }
        if wrong.is_empty() {
            Ok(out)
        } else {
            Err(Error::several(wrong))
        }
    }
}

/// What a plan is asked for.
///
/// An operation produces an artefact first, and the front end keeps it
/// without writing anything. Yet the form asked, for that gesture, everything
/// the **writing** needs — a frame, a title, a data file — and the module
/// refused a plan without them. The two halves of a plan were
/// already there; this is the switch that lets a caller ask for one.
///
/// `Geometry` relaxes the slots outside the `operation` group and lets a
/// frameless plan come back **blocked** rather than refused; `Writing` is what
/// `Registry::plan` has always done. The CLI chains the two; the front end keeps
/// them apart, which is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The shapes, to keep in a scene — nothing will be written from this plan.
    Geometry,
    /// The objects, to apply.
    Writing,
}

/// A list answer: the shape, then the count, then each element.
///
/// In that order on purpose. A caller who sent a bare value has made a *shape*
/// mistake, and telling them "0 values, at least 1 needed" would send them
/// looking for a missing answer they did in fact give — the refusal names what
/// arrived and what was expected instead.
fn check_many(slot: &Slot, value: &Value) -> Vec<Refusal> {
    let Some(items) = value.as_array() else {
        return vec![Refusal::about(
            "shape",
            slot.name,
            format!(
                "`{}` takes several values, so its answer is a list — `[…]`, not a lone {}",
                slot.name,
                kind_of(value)
            ),
        )];
    };
    let count = items.len() as u32;
    if count < slot.least {
        return vec![Refusal::about(
            "count",
            slot.name,
            format!(
                "`{}` needs at least {} value(s) and has {count}: {}",
                slot.name, slot.least, slot.about
            ),
        )];
    }
    if let Some(most) = slot.most
        && count > most
    {
        return vec![Refusal::about(
            "count",
            slot.name,
            format!("`{}` takes at most {most} value(s) and has {count}", slot.name),
        )];
    }
    // Each one, and **every** one: stopping at the first would make correcting a
    // list a matter of as many round trips as it has bad entries.
    items
        .iter()
        .enumerate()
        .filter_map(|(at, item)| slot.need.accepts(item).err().map(|why| (at, why)))
        .map(|(at, why)| Refusal::about("value", slot.name, format!("`{}`[{at}]: {why}", slot.name)))
        .collect()
}

/// What a JSON value is, for a refusal that names what arrived.
fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "flag",
        Value::Number(_) => "number",
        Value::String(_) => "text",
        Value::Array(_) => "list",
        Value::Object(_) => "object",
    }
}

/// Can the operation work on this bag?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fitness {
    /// Yes.
    Fit,
    /// No, and here is why — named at both ends, and under the input slot
    /// it is about when it is about one.
    Unfit(Refusal),
}

impl Fitness {
    /// `true` when fit.
    pub fn is_fit(&self) -> bool {
        matches!(self, Fitness::Fit)
    }
}

/// One object the plan will write.
#[derive(Debug, Clone, PartialEq)]
pub struct NewObject {
    /// Already assigned — which is what lets another object of the same plan
    /// reference this one before anything exists.
    pub uuid: String,
    /// `resqml22.PointSetRepresentation`.
    pub qualified_type: String,
    /// What this object is *for* in the plan — `representation`, `crs`,
    /// `trajectory` — for a reader of the plan and for the activity.
    pub role: String,
    /// Its title, as written in its citation.
    pub title: String,
    /// The object, in the workspace's JSON encoding — what `put_json` takes.
    pub json: Value,
}

impl NewObject {
    /// The `objectVersion` the object declares, read where its release keeps it
    /// — `objectVersion` on the root (eml 2.1 and later) or
    /// `Citation.VersionString` (eml 2.0) — as the reader of the package reads
    /// it; blank is none.
    pub fn object_version(&self) -> Option<String> {
        let text = |value: &Value| {
            value
                .as_str()
                .map(|one| one.trim().to_string())
                .filter(|one| !one.is_empty())
        };
        text(&self.json["objectVersion"]).or_else(|| text(&self.json["Citation"]["VersionString"]))
    }

    /// **Its address once written**: the full uri, version included when it
    /// declares one — what the store will answer for it
    /// ([`energyml_core::identity::ObjectKey::etp_uri`]). A plan names an object
    /// by this, never by its uuid: a plan may write a second version beside the
    /// first ([`NEW_VERSION_ROLE`]), and both have the one uuid.
    pub fn uri(&self) -> String {
        crate::pieces::Ref::new(self.uuid.clone(), self.qualified_type.clone(), self.title.clone())
            .with_version(self.object_version())
            .uri()
    }

    /// The reference to it, the version it declares included.
    pub fn reference(&self) -> crate::pieces::Ref {
        crate::pieces::Ref::new(self.uuid.clone(), self.qualified_type.clone(), self.title.clone())
            .with_version(self.object_version())
    }
}

/// The [`NewObject::role`] of an object written **as another version** of one
/// the package holds: same uuid, same type, another `objectVersion`.
///
/// `apply` writes it through `EpcStore::put_as_new_version`, the one gate that
/// adds a version — an ordinary `put` replaces, so that a version is never made
/// by accident. Carried by the role rather
/// than by a field of its own because the role is what says, to a reader of the
/// plan and to the activity, *what this object is for in the plan*: here, to
/// stand beside another. If the exact identity — uuid, type and version — is
/// already in the package the gate refuses, and `apply` reports it by name.
pub const NEW_VERSION_ROLE: &str = "new-version";

/// The numbers a dataset holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Numbers {
    /// Floating point.
    F64(Vec<f64>),
    /// Whole.
    I64(Vec<i64>),
}

impl Numbers {
    /// How many values.
    pub fn len(&self) -> usize {
        match self {
            Numbers::F64(v) => v.len(),
            Numbers::I64(v) => v.len(),
        }
    }

    /// No value at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An array the plan will write — the values, not a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    /// The external file, relative to the package.
    pub file: String,
    /// The path inside it.
    pub path: String,
    /// The shape, row-major.
    pub dimensions: Vec<i64>,
    /// The numbers.
    pub values: Numbers,
}

/// A file the plan puts **in the package**, beside its objects — a part that
/// is not an object: a README, a PDF, an export.
///
/// Energistics Packaging Conventions v1.0: such files
/// are foreseen, "As a guideline … stored in a folder named `media`" (§3.2
/// pp. 11–12); `[Content_Types].xml` types each part (§2.2.1 p. 9); and the
/// object a file shows is tied to it by a relationship, `destinationMedia` from
/// the object and `sourceMedia` back (§3.4.2 pp. 15–16) — never by the content
/// type, which says *what* the part is and not *which object* it is about.
///
/// [`crate::apply`] writes it **after** the objects, so that an object of the
/// same plan can be related to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    /// The part name — `media/README.md`.
    pub path: String,
    /// Its content type — `text/markdown`. Stated, never guessed.
    pub content_type: String,
    /// The bytes, exactly.
    pub bytes: Vec<u8>,
    /// The uuids of the objects it is a media representation of — in the
    /// package already, or written by the same plan.
    pub related_to: Vec<String>,
}

/// What an operation has **computed** — a node of the artefact tree.
///
/// This is the "computed" half of a plan, made explicit: *an operation first
/// produces an artefact, which will be promoted into objects*. The "writing"
/// half — the objects, their uuids, their CRS — stays beside it, and can be
/// blocked without this one being so.
///
/// # Why it is **carried** and not derived
///
/// Guessing the geometry from the plan's objects would require a second reader
/// of RESQML, and it would not suffice: a `PolylineSetRepresentation` writes its
/// polyline lengths **in its JSON** (`NodeCountPerPolyline`), not in
/// an array. The topology therefore cannot be reread from the datasets, and deducing it from the
/// name of a class is exactly what this repository refuses elsewhere.
///
/// # The shape is the source, the array is a view of it
///
/// The points are **not** here *and* in the `Dataset`: operations
/// build the shape, then draw the dataset's values from it
/// (`support::flat_points`). The reverse — keeping both side by side — would make a plan of a
/// million points carry them twice, and above all would open the door to
/// their diverging.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    /// Its identity **in the plan** — `0`, `1`. A rank, not a uuid: the uuid
    /// belongs to the object, which exists only at writing.
    pub id: String,
    /// The node this one is a part of, `None` for a root.
    ///
    /// An operation produces a tree, not a list: a fence
    /// will return its master section and its panels under a root. All thirteen
    /// operations today return roots, and that is the honest answer —
    /// none has sub-parts.
    pub parent: Option<String>,
    /// What is read on the scene's line.
    pub title: String,
    /// The geometry, in the package's frame.
    pub geometry: energyml_geometry::geometry::Geometry,
    /// The uuid of the object that writing would make of it, when the plan writes one.
    ///
    /// This is what links the two halves without going through the titles, which
    /// are not unique. `None` for what is computed and has no object.
    pub object: Option<String>,
}

/// Something the plan wants said — a key and its detail, for a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// A stable word: `closed-detected`, `reprojected`.
    pub key: &'static str,
    /// What happened, in English.
    pub detail: String,
}

/// Inputs the plan set aside, counted and named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// How many.
    pub count: usize,
    /// Why, in English.
    pub why: String,
}

/// What the activity will record, beyond its outputs — those come from the
/// objects written.
#[derive(Debug, Clone)]
pub struct ActivityDraft {
    /// The canonical recipe, a key of `rc/activityTemplates.json`.
    pub recipe: &'static str,
    /// The activity's title.
    pub title: String,
    /// What names this run — a source file and a stage, an input uuid — so that
    /// the activity's uuid is derived from it and the instant, never drawn.
    pub seed: String,
    /// The parameters: the choices that mattered, and what was measured.
    pub parameters: Vec<energyml_domain::provenance::Parameter>,
}

/// What an operation decided: several objects, coherent, with their identities
/// already known — and nothing done.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The objects, in the order they should be written.
    pub objects: Vec<NewObject>,
    /// The arrays.
    pub datasets: Vec<Dataset>,
    /// The files put in the package beside the objects — see [`Media`]. Empty
    /// for every operation but `media`, and a plan without any writes exactly
    /// what it wrote before this field existed.
    pub media: Vec<Media>,
    /// The activity to record.
    pub activity: ActivityDraft,
    /// What the plan wants said.
    pub says: Vec<Note>,
    /// What was set aside.
    pub dropped: Vec<Dropped>,
    /// **What the operation has computed**, as a tree of artefacts — see [`Made`].
    ///
    /// Empty means *this operation computes no geometry*, which is
    /// true of `crs`, `property` and `colormap`. A plan that writes a
    /// representation and carries no shape is a defect, and
    /// `every_plan_that_writes_a_shape_carries_it` says so.
    pub shapes: Vec<Made>,
    /// **What would be needed to write**, when something is missing.
    ///
    /// This is the second half of a plan: computing a geometry requires no CRS,
    /// writing the object that carries it does. An operation that has computed but
    /// will not be able to write **succeeds** by saying so, instead of failing —
    /// otherwise it refuses to compute for a reason that concerns only writing,
    /// which is the defect once reported on `outline` and a real dataset.
    ///
    /// `None` is the ordinary case. `Some` means: what is here is good to
    /// look at and to keep in the scene, [`crate::apply::apply`] will refuse it.
    ///
    /// **It is not the same thing as an empty plan**, and confusing them would cost
    /// the same mistake as `undefinedNodes: 0` — *nothing to write* and *nothing could
    /// be written* are two pieces of news.
    pub blocked: Option<Error>,
}

impl Plan {
    /// The objects of that role.
    pub fn of_role<'a>(&'a self, role: &'a str) -> impl Iterator<Item = &'a NewObject> + 'a {
        self.objects.iter().filter(move |o| o.role == role)
    }

    /// Nothing to write — no object and no file.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.media.is_empty()
    }

    /// What the plan will do, for somebody to read before it is applied — the
    /// objects, what was said and set aside, the arrays' shapes. The arrays'
    /// **values** are not here: they travel as buffers, or not at all.
    pub fn summary_json(&self) -> Value {
        serde_json::json!({
            "title": self.activity.title,
            "recipe": self.activity.recipe,
            // `uri` is the key — the full identity, version included; `uuid` and
            // `objectVersion` are its parts, kept for the readers written before it.
            "objects": self.objects.iter().map(|o| serde_json::json!({
                "role": o.role, "type": o.qualified_type, "uri": o.uri(), "uuid": o.uuid, "objectVersion": o.object_version(), "title": o.title,
            })).collect::<Vec<_>>(),
            "datasets": self.datasets.iter().map(|d| serde_json::json!({
                "file": d.file, "path": d.path, "dimensions": d.dimensions, "count": d.values.len(),
            })).collect::<Vec<_>>(),
            // The files' **bytes** are not here either — their size is.
            "media": self.media.iter().map(|m| serde_json::json!({
                "path": m.path, "contentType": m.content_type, "bytes": m.bytes.len(), "relatedTo": m.related_to,
            })).collect::<Vec<_>>(),
            "blocked": self.blocked.as_ref().map(ToString::to_string),
            "says": self.says.iter().map(|n| serde_json::json!({ "key": n.key, "detail": n.detail })).collect::<Vec<_>>(),
            "dropped": self.dropped.iter().map(|d| serde_json::json!({ "count": d.count, "why": d.why })).collect::<Vec<_>>(),
        })
    }
}

/// How the objects a plan makes are named.
#[derive(Debug, Clone)]
pub enum Identity {
    /// uuid v5 of `(namespace, operation, input identity, role)` — the choice
    /// a version translation and the SIGEOL import made, for the same reason: two
    /// runs of the same file write the same package, so a diff says whether the
    /// data moved or only its names did.
    Derived {
        /// The namespace to hang from — [`crate::naming::NAMESPACE`] for the
        /// built-ins.
        namespace: Uuid,
    },
    /// uuids supplied by the caller, in the order the plan asks for them — the
    /// front end, which draws them from `crypto`.
    Given(Vec<String>),
}

impl Identity {
    /// The built-in derivation.
    pub fn derived() -> Self {
        Identity::Derived {
            namespace: crate::naming::NAMESPACE,
        }
    }

    /// The uuid of rank `rank` in the plan, seeded by `name` — the derivation
    /// ignores the rank, the given list ignores the name.
    pub fn uuid(&self, name: &str, rank: usize) -> Result<String, Error> {
        match self {
            Identity::Derived { namespace } => Ok(Uuid::new_v5(namespace, name.as_bytes()).to_string()),
            Identity::Given(list) => list.get(rank).cloned().ok_or_else(|| {
                Error::new(format!(
                    "the plan needs at least {} uuid(s) and {} were given",
                    rank + 1,
                    list.len()
                ))
            }),
        }
    }
}

/// When the plan is made — an input, so that two equal runs write equal
/// packages, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct When {
    /// `YYYY-MM-DDThh:mm:ssZ`.
    pub creation: String,
    /// Who is writing, for every `Citation`.
    pub originator: String,
}

/// The typed side of an operation — see [`crate::typed`].
pub use crate::typed::{About, Context, Feedback, Operation, Silent, Typed, typed};

/// Why an operation could not plan — **one refusal or several**, each with
/// its key and the slot it is about.
///
/// `settle` finds every wrong answer of a form at once, and a caller who could
/// only read them as one sentence joined by `;` could not put each under its
/// field. The sentence is
/// still there — [`Error::message`], and `Display` — for the CLI, which prints.
///
/// Never empty: an error that refuses nothing would read as a success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    refusals: Vec<Refusal>,
}

impl Error {
    /// A refusal about no slot in particular.
    pub fn new(message: impl Into<String>) -> Self {
        Error {
            refusals: vec![Refusal::new("refused", message)],
        }
    }

    /// Several refusals, found together.
    pub fn several(refusals: Vec<Refusal>) -> Self {
        assert!(!refusals.is_empty(), "an error refuses something");
        Error { refusals }
    }

    /// Each refusal, in the order it was found.
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }

    /// The refusals as one sentence — what a terminal prints.
    pub fn message(&self) -> String {
        self.refusals
            .iter()
            .map(|one| one.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// A refusal, **structured**: a stable key, the slot it is about when there is
/// one, and the message.
///
/// The key and the slot are what let a front end translate a refusal and show it
/// under its field; the message is what the CLI prints. [`Error`] is its plain
/// form, and each converts into the other so that an operation written
/// against one calls code written against the other with a `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// A stable word — what a front end translates. `slot` for a refusal an
    /// operation raised about one of its slots; from `settle`, `required`,
    /// `value`, `count`, `shape` (a bare value where a list is wanted),
    /// `inapplicable` (an answer to a slot that cannot act here) and
    /// `declaration` (the operation's own fault); `unfit` when the bag is not
    /// one the operation takes; `cancelled`; `refused` for the rest.
    pub key: &'static str,
    /// The slot it is about, when it is about one.
    pub slot: Option<String>,
    /// What is wrong, in English.
    pub message: String,
}

impl Refusal {
    /// A refusal under a key.
    pub fn new(key: &'static str, message: impl Into<String>) -> Self {
        Refusal {
            key,
            slot: None,
            message: message.into(),
        }
    }

    /// A refusal about one slot, its name leading the message.
    pub fn on(slot: &str, why: impl fmt::Display) -> Self {
        Refusal {
            key: "slot",
            slot: Some(slot.to_string()),
            message: format!("`{slot}`: {why}"),
        }
    }

    /// A refusal about one slot, under a key, its message as written.
    pub fn about(key: &'static str, slot: &str, message: impl Into<String>) -> Self {
        Refusal {
            key,
            slot: Some(slot.to_string()),
            message: message.into(),
        }
    }

    /// `{ key, slot, message }` — the one wire form, which the front end and any
    /// other caller read; `slot` is `null` when the refusal is about none.
    pub fn json(&self) -> Value {
        serde_json::json!({ "key": self.key, "slot": self.slot, "message": self.message })
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

/// One refusal stays itself; several become one sentence, which is all a
/// single refusal can hold.
impl From<Error> for Refusal {
    fn from(mut error: Error) -> Self {
        if error.refusals.len() == 1 {
            error.refusals.remove(0)
        } else {
            Refusal::new("refused", error.message())
        }
    }
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        Error {
            refusals: vec![refusal],
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

/// **One kind of object an operation writes, by its role** — what
/// [`AnyOperation::gives`] declares, the other side of [`AnyOperation::takes`].
///
/// Chaining operations in a workflow requires that operations expose the objects they created. The role is the
/// one `NewObject::role` carries in a plan — `apply` groups by it into the run's
/// collections, the run manifest lists by it, a Galaxy tool gets one output per
/// role, and the next step names it. So a role a plan writes and the declaration
/// does not name is a defect of the operation, held by a test.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// `representation`, `frame`, `category-lookup` — `NewObject::role`.
    pub role: &'static str,
    /// The qualified types written under it (`resqml22.PointSetRepresentation`).
    pub types: &'static [&'static str],
    /// At least this many on a run that is not refused.
    pub least: usize,
    /// At most this many; `None` is unbounded.
    pub most: Option<usize>,
    /// One line, English: what these objects are, for a form and a Galaxy tool.
    pub describe: &'static str,
}

/// A way of creating objects — **the erased interface** the registry, the
/// front end's module and the CLI execute.
///
/// An operation written against typed parameters and inputs implements
/// [`Operation`] and comes here through [`typed`]; one not yet migrated
/// implements this directly. Callers never tell the two apart.
pub trait AnyOperation: Send + Sync {
    /// `pointset`, `crs`, `sigeol-units`.
    fn name(&self) -> &'static str;
    /// One line, for `--list`.
    fn describe(&self) -> &'static str;
    /// The qualified types it makes.
    fn makes(&self) -> &'static [&'static str];
    /// The canonical activity template it records under — a key of
    /// `rc/activityTemplates.json`.
    fn recipe(&self) -> &'static str;
    /// Can it work on this bag? A named refusal, and an empty bag is a
    /// legitimate input — `crs` takes nothing.
    fn fitness(&self, pieces: &Pieces) -> Fitness;
    /// What it still needs to be told, with what it proposes. Must answer on
    /// **any** bag, the empty one included — that is what the template is
    /// projected from; suggestions are simply absent then.
    fn needs(&self, pieces: &Pieces) -> Vec<Slot>;

    /// **What the operation takes** — its inputs, declared.
    ///
    /// Empty by default, and that is an answer: `crs` takes nothing. Each
    /// slot carries its cardinality and its order like any `Slot`,
    /// so a `fence` will say *one trace* as `1..1` and *surfaces* as `2..n`
    /// ordered without a form having to guess.
    ///
    /// **Without an argument, unlike [`AnyOperation::needs`]**: what an
    /// operation takes does not depend on what it has already been given — that is where
    /// the form is rendered from, before any choice. A slot that
    /// would only appear afterwards is what [`Applies`] covers, and that is
    /// declared rather than guessed.
    fn takes(&self) -> Vec<Slot> {
        Vec::new()
    }
    /// The feature and the interpretation this operation writes when none is
    /// named — `(feature type, interpretation type)`, or `None` when it never
    /// makes them.
    ///
    /// Declared here and **built** in `ops::support`, on purpose: a form needs
    /// to say *create one / pick one* before anything is planned, and the
    /// types are what it says. The construction needs the choices, which a
    /// declaration must not.
    fn represents(&self) -> Option<(&'static str, &'static str)> {
        None
    }
    /// What the activity records that is neither a choice nor an output — a
    /// count measured while planning. Empty by default.
    fn reports(&self) -> Vec<Slot> {
        Vec::new()
    }
    /// **What it writes, by role** — see [`Output`]. Empty by default, which
    /// means *not declared yet*, never *writes nothing*: a caller that chains
    /// steps falls back to the roles the plan carries.
    fn gives(&self) -> Vec<Output> {
        Vec::new()
    }
    /// The plan. `choices` have been [settled](Choices::settle) already.
    fn plan(
        &self,
        pieces: &Pieces,
        choices: &Choices,
        identity: &Identity,
        when: &When,
    ) -> Result<Plan, Error>;

    /// The plan, with its whole [`Context`] — the purpose and the feedback
    /// beside the identity and the instant. What the registry calls; an
    /// operation that does not read the context need not say anything.
    fn plan_with(&self, pieces: &Pieces, choices: &Choices, cx: &Context<'_>) -> Result<Plan, Error> {
        self.plan(pieces, choices, cx.identity, cx.when)
    }

    /// Where a listing ranges it — see [`Category`]. Every built-in answers;
    /// the default is for an operation brought by a caller.
    fn category(&self) -> Category {
        Category::Other
    }

    /// **The other categories it is also listed under** — none by default.
    ///
    /// Some operations belong in several categories — for example the one that
    /// extracts wellhead points to create a point set is filed under "write" but
    /// could also be under "Wells". Declared here
    /// rather than guessed by the front end from what the operation takes: a slot of
    /// shapes carries no type to guess from, and the module is where an
    /// operation says what it is.
    fn also(&self) -> &'static [Category] {
        &[]
    }

    /// The operation as a listing sees it: name, description, what it makes.
    fn json(&self) -> Value {
        serde_json::json!({
            "name": self.name(),
            "describe": self.describe(),
            "makes": self.makes(),
            "recipe": self.recipe(),
            "category": self.category().key(),
            "also": self.also().iter().map(|one| one.key()).collect::<Vec<_>>(),
        })
    }
}

/// Where an operation is ranged in a listing — the groups of QGIS Processing,
/// the paths of GDAL's `gdal raster …`.
/// Nineteen operations in one list was the complaint.
///
/// A closed list on purpose: a group a front end has no word for would show as a
/// raw key, and a front end's test holds it to one label per key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// A representation made from a shape or a file, copied as it is.
    Create,
    /// A frame of reference.
    Frame,
    /// A new geometry decided from the input — a triangulation, a grid, a line.
    Geometry,
    /// What is measured on a shape and written as a property.
    Analysis,
    /// Faults: their traces, their surfaces, what they cut.
    Faults,
    /// Properties and their colours.
    Properties,
    /// Wells.
    Wells,
    /// A foreign vocabulary brought in.
    Import,
    /// **Values no input carries, invented to build a synthetic model** — a
    /// horizon laid by a thickness law, logs by a gradient, a doublet's rates,
    /// a water sample, a fiber, an injection test. Every object such an
    /// operation writes is marked `synthetic = true`.
    ///
    /// A category of their own, and **only** this one — no `also` into a
    /// general category: these operations belong in the general list only when they
    /// allow geometric operations useful to other, more generic use cases; otherwise
    /// they get a category of their own. The test is what the operation
    /// **does**, not who asked for it first: `time-series`, `well-from-plan`,
    /// `business-associate` were written for Lutetia and are general;
    /// `fault-throw` and `witsml-wells` invent only when told to (their
    /// `synthetic` parameter) and stay where they are.
    Synthetic,
    /// An operation brought by a caller, which did not say.
    Other,
}

impl Category {
    /// Every category, in the order a listing shows them.
    pub const ALL: &'static [Category] = &[
        Category::Create,
        Category::Frame,
        Category::Geometry,
        Category::Analysis,
        Category::Faults,
        Category::Properties,
        Category::Wells,
        Category::Import,
        Category::Synthetic,
        Category::Other,
    ];

    /// The stable word a front end translates.
    pub fn key(self) -> &'static str {
        match self {
            Category::Create => "create",
            Category::Frame => "frame",
            Category::Geometry => "geometry",
            Category::Analysis => "analysis",
            Category::Faults => "faults",
            Category::Properties => "properties",
            Category::Wells => "wells",
            Category::Import => "import",
            Category::Synthetic => "synthetic",
            Category::Other => "other",
        }
    }
}

/// What the registry says of one operation for a given bag.
pub struct Offer<'a> {
    /// The operation.
    pub operation: &'a dyn AnyOperation,
    /// Fit, or why not.
    pub fitness: Fitness,
    /// What it would ask.
    pub needs: Vec<Slot>,
}

/// The operations, built-ins by default, the caller's tried **first** so that
/// a built-in is replaceable and not only extensible.
pub struct Registry {
    operations: Vec<Box<dyn AnyOperation>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// The built-in operations.
    pub fn new() -> Self {
        Self {
            operations: crate::ops::builtin(),
        }
    }

    /// No operation at all — for a caller that brings its own.
    pub fn bare() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Add one, ahead of the built-ins.
    pub fn register(&mut self, operation: Box<dyn AnyOperation>) {
        self.operations.insert(0, operation);
    }

    /// Every operation, the caller's first.
    pub fn iter(&self) -> impl Iterator<Item = &dyn AnyOperation> {
        self.operations.iter().map(|o| o.as_ref())
    }

    /// The first operation of that name.
    pub fn get(&self, name: &str) -> Option<&dyn AnyOperation> {
        self.iter().find(|o| o.name() == name)
    }

    /// *What can I create with this?* — every operation, fit or not, with its
    /// reason and what it would ask.
    pub fn offers(&self, pieces: &Pieces) -> Vec<Offer<'_>> {
        self.iter()
            .map(|operation| Offer {
                operation,
                fitness: operation.fitness(pieces),
                needs: operation.needs(pieces),
            })
            .collect()
    }

    /// Plan with the named operation: fitness first, then the settled choices,
    /// then the plan.
    pub fn plan(
        &self,
        name: &str,
        pieces: &Pieces,
        choices: &Choices,
        identity: &Identity,
        when: &When,
    ) -> Result<Plan, Error> {
        self.plan_for(name, pieces, choices, identity, when, Purpose::Writing)
    }

    /// Plan for a **purpose** — the geometry alone, or the writing.
    pub fn plan_for(
        &self,
        name: &str,
        pieces: &Pieces,
        choices: &Choices,
        identity: &Identity,
        when: &When,
        purpose: Purpose,
    ) -> Result<Plan, Error> {
        let cx = Context {
            identity,
            when,
            purpose,
            feedback: &Silent,
            bag: pieces,
        };
        self.plan_in(name, pieces, choices, &cx)
    }

    /// Plan in a whole [`Context`] — a caller that listens for progress, or
    /// may ask the computation to stop.
    pub fn plan_in(
        &self,
        name: &str,
        pieces: &Pieces,
        choices: &Choices,
        cx: &Context<'_>,
    ) -> Result<Plan, Error> {
        // One span per operation run: the log says which one is slow, and how
        // many objects and arrays it came to. Never an event per element.
        let span = tracing::info_span!(
            "operation.plan",
            operation = name,
            objects = tracing::field::Empty,
            datasets = tracing::field::Empty
        );
        let _entered = span.enter();
        let operation = self
            .get(name)
            .ok_or_else(|| Error::new(format!("no operation named `{name}`")))?;
        if let Fitness::Unfit(why) = operation.fitness(pieces) {
            return Err(Refusal {
                message: format!("`{name}` is not fit for this input: {}", why.message),
                ..why
            }
            .into());
        }
        let slots = operation.needs(pieces);
        let given = crate::declare::given_inputs(pieces, &operation.takes());
        let settled = choices.settle_given(&slots, cx.purpose, &given)?;
        let mut plan = operation.plan_with(pieces, &settled, cx)?;
        record_choices(&mut plan, &slots, &settled);
        span.record("objects", plan.objects.len());
        span.record("datasets", plan.datasets.len());
        Ok(plan)
    }
}

/// Write every settled choice into the activity, under its slot's name.
///
/// # Why this is not each operation's business
///
/// It was, and that is the defect: an operation recorded the parameters it
/// thought interesting, so an activity said `sourceEpsg` and not `ground`, and
/// **what the run was told could not be read back**. An activity that does not
/// record its inputs is not a provenance, it is a receipt.
///
/// Doing it here rather than in `plan` also keeps the operations pure of the
/// question: the registry is the one place that holds the *settled* choices —
/// the answers **and** the suggestions that stood in for what was left out,
/// which is exactly what a second run would have to be given to come out the
/// same.
///
/// # What it does not overwrite
///
/// A parameter the operation already wrote under the same title stays. It knows
/// things this pass cannot — the uom of a measure, an object's full reference —
/// and two parameters of one title would be an activity saying a thing twice.
///
/// Titles are slot names, so they are all titles the template declares; this
/// cannot make an activity name a parameter its template does not have.
fn record_choices(plan: &mut Plan, slots: &[Slot], settled: &Choices) {
    use energyml_domain::provenance::Parameter;
    for slot in slots {
        let Some(value) = settled.get(slot.name) else {
            continue;
        };
        if plan.activity.parameters.iter().any(|p| title_of(p) == slot.name) {
            continue;
        }
        let parameter = match &slot.need {
            Need::Object { .. } | Need::Crs | Need::Interpretation | Need::PropertyKind => {
                // A bare uuid is not a reference, and inventing the rest of one
                // here would be this pass asserting a content type nobody wrote.
                if !value.is_object() {
                    continue;
                }
                Parameter::Object {
                    title: slot.name.to_string(),
                    reference: value.clone(),
                }
            }
            // **An input is not a parameter.** The activity records its
            // inputs as such, through `apply`; adding it here would make it a
            // setting, and the same fact would be stated twice under two
            // natures.
            Need::Input(_) => continue,
            Need::Number { uom, .. } => match value.as_f64() {
                Some(number) => Parameter::Double {
                    title: slot.name.to_string(),
                    value: number,
                    uom: uom.map(str::to_string),
                },
                None => continue,
            },
            Need::Epsg | Need::Integer { .. } => match value.as_f64() {
                Some(number) => Parameter::Double {
                    title: slot.name.to_string(),
                    value: number,
                    uom: None,
                },
                None => continue,
            },
            // eml 2.3 has no boolean parameter — the template's `Constraint`
            // says which two words, and this writes one of them.
            Need::Flag => Parameter::Text {
                title: slot.name.to_string(),
                value: if value.as_bool() == Some(true) {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            },
            Need::Text | Need::Title | Need::Choice(_) | Need::Type(_) | Need::Vocabulary(_) => {
                match value.as_str() {
                    Some(text) => Parameter::Text {
                        title: slot.name.to_string(),
                        value: text.to_string(),
                    },
                    None => continue,
                }
            }
        };
        plan.activity.parameters.push(parameter);
    }
}

/// A parameter's title, whatever its kind.
fn title_of(parameter: &energyml_domain::provenance::Parameter) -> &str {
    use energyml_domain::provenance::Parameter;
    match parameter {
        Parameter::Double { title, .. } | Parameter::Text { title, .. } | Parameter::Object { title, .. } => {
            title
        }
    }
}
