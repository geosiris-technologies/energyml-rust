//! Parameters and inputs **declared once**, as Rust types — the runtime the
//! derives of `energyml-build-derive` write against.
//!
//! The slots a form and
//! a template read are still [`Slot`]s — the wire does not change — but they
//! are *generated* from a struct, and the same struct is what `plan` receives.
//! A parameter declared in `needs()` and read back by its name in `plan()`,
//! its default written twice, is what this replaces.
//!
//! # The pieces
//!
//! - [`Scalar`] — one answer: how it is asked ([`Need`]), read and written;
//! - [`Param`] — a field: a scalar, `Option<T>` (may be absent) or `Vec<T>`
//!   (several) — the cardinality comes from the type;
//! - [`Params`] — a struct of fields, or a union of cases ([`union_slots`]);
//! - [`Input`], [`InputParam`], [`Inputs`] — the same for what an operation
//!   **takes**, and [`Binder`], the one place that knows how pieces are named.

use energyml_domain::color::Hsv;
use energyml_geometry::geometry::Geometry;

use crate::geojson::Feature;
use crate::operation::{Applies, Need, Refusal, Slot, Takes};
use crate::pieces::{Known, Lattice, Pieces, Ref, Source};

/// JSON, as the choices hold it — re-exported for the generated code.
pub use serde_json::Value as Json;

/// The derives, beside the traits they implement — the way serde puts
/// `Serialize` the trait and `Serialize` the derive under one name.
pub use energyml_build_derive::{Choice, Inputs, Params};

// ---------------------------------------------------------------- scalars

/// One answer: how it is asked, read and written.
pub trait Scalar: Sized {
    /// The kind of answer — what a form renders and a template projects.
    fn need() -> Need;
    /// The answer, from its JSON — or what is wrong with it.
    fn from_json(value: &Json) -> Result<Self, String>;
    /// The answer as JSON — what a default suggests.
    fn to_json(&self) -> Json;
}

/// A field of a parameter struct: a scalar, `Option<T>` or `Vec<T>`.
pub trait Param: Sized {
    /// The kind of answer.
    fn need() -> Need;
    /// How many values at least — `MinOccurs`.
    fn least() -> u32 {
        1
    }
    /// How many at most — `MaxOccurs`, `None` for no ceiling.
    fn most() -> Option<u32> {
        Some(1)
    }
    /// The field, from its settled answer — `None` when there is none.
    fn read(value: Option<&Json>) -> Result<Self, String>;
    /// The value as JSON, `None` for an absent one.
    fn json(&self) -> Option<Json>;
}

/// `Param` for a scalar — one value, required. A macro and not a blanket
/// implementation, so that `Option<T>` and `Vec<T>` can have theirs.
#[macro_export]
macro_rules! scalar_param {
    ($($ty:ty),* $(,)?) => {$(
        impl $crate::declare::Param for $ty {
            fn need() -> $crate::operation::Need {
                <$ty as $crate::declare::Scalar>::need()
            }
            fn read(value: ::std::option::Option<&$crate::declare::Json>) -> ::std::result::Result<Self, ::std::string::String> {
                match value {
                    ::std::option::Option::Some(value) => <$ty as $crate::declare::Scalar>::from_json(value),
                    ::std::option::Option::None => ::std::result::Result::Err("is required and has no value".to_string()),
                }
            }
            fn json(&self) -> ::std::option::Option<$crate::declare::Json> {
                ::std::option::Option::Some($crate::declare::Scalar::to_json(self))
            }
        }
    )*};
}

impl<T: Scalar> Param for Option<T> {
    fn need() -> Need {
        T::need()
    }
    fn least() -> u32 {
        0
    }
    fn read(value: Option<&Json>) -> Result<Self, String> {
        value.map(T::from_json).transpose()
    }
    fn json(&self) -> Option<Json> {
        self.as_ref().map(T::to_json)
    }
}

impl<T: Scalar> Param for Vec<T> {
    fn need() -> Need {
        T::need()
    }
    fn least() -> u32 {
        0
    }
    fn most() -> Option<u32> {
        None
    }
    fn read(value: Option<&Json>) -> Result<Self, String> {
        match value {
            None => Ok(Vec::new()),
            Some(Json::Array(items)) => items
                .iter()
                .enumerate()
                .map(|(at, item)| T::from_json(item).map_err(|why| format!("[{at}]: {why}")))
                .collect(),
            Some(other) => Err(format!("takes several values, so a list — not {other}")),
        }
    }
    fn json(&self) -> Option<Json> {
        Some(Json::Array(self.iter().map(T::to_json).collect()))
    }
}

impl Scalar for f64 {
    fn need() -> Need {
        Need::Number {
            uom: None,
            least: None,
            most: None,
        }
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        value
            .as_f64()
            .ok_or_else(|| format!("expected a number, got {value}"))
    }
    fn to_json(&self) -> Json {
        Json::from(*self)
    }
}

/// A whole number — an integral float accepted, because EML 2.3 has no
/// integer parameter and a code read back from an activity is a `double`.
fn whole(value: &Json) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.fract() == 0.0 && f.is_finite())
            .map(|f| f as i64)
    })
}

impl Scalar for i64 {
    fn need() -> Need {
        Need::Integer {
            least: None,
            most: None,
        }
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        whole(value).ok_or_else(|| format!("expected a whole number, got {value}"))
    }
    fn to_json(&self) -> Json {
        Json::from(*self)
    }
}

impl Scalar for usize {
    fn need() -> Need {
        Need::Integer {
            least: Some(0),
            most: None,
        }
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        whole(value)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| format!("expected a whole number, zero or more, got {value}"))
    }
    fn to_json(&self) -> Json {
        Json::from(*self)
    }
}

impl Scalar for String {
    fn need() -> Need {
        Need::Text
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("expected text, got {value}"))
    }
    fn to_json(&self) -> Json {
        Json::from(self.as_str())
    }
}

impl Scalar for bool {
    fn need() -> Need {
        Need::Flag
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        // A flag read back from an activity is text: EML 2.3 has no boolean.
        match value {
            Json::Bool(flag) => Ok(*flag),
            Json::String(text) if text == "true" => Ok(true),
            Json::String(text) if text == "false" => Ok(false),
            other => Err(format!("expected true or false, got {other}")),
        }
    }
    fn to_json(&self) -> Json {
        Json::from(*self)
    }
}

/// A title for the objects made — one per plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(pub String);

impl Scalar for Title {
    fn need() -> Need {
        Need::Title
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        String::from_json(value).map(Title)
    }
    fn to_json(&self) -> Json {
        Json::from(self.0.as_str())
    }
}

/// An EPSG code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Epsg(pub i64);

impl Scalar for Epsg {
    fn need() -> Need {
        Need::Epsg
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        whole(value)
            .filter(|v| *v > 0)
            .map(Epsg)
            .ok_or_else(|| format!("expected an EPSG code, got {value}"))
    }
    fn to_json(&self) -> Json {
        Json::from(self.0)
    }
}

/// An object of the package, by reference: a uuid, or a `DataObjectReference`.
macro_rules! reference {
    ($(#[$doc:meta] $name:ident => $need:expr),* $(,)?) => {$(
        #[$doc]
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name(pub Json);

        impl Scalar for $name {
            fn need() -> Need {
                $need
            }
            fn from_json(value: &Json) -> Result<Self, String> {
                <Self as Scalar>::need().accepts(value).map(|()| $name(value.clone()))
            }
            fn to_json(&self) -> Json {
                self.0.clone()
            }
        }
    )*};
}

reference! {
    /// Any object of the package.
    ObjectRef => Need::Object { types: &[] },
    /// A local CRS of the package.
    CrsRef => Need::Crs,
    /// An interpretation of the package.
    InterpretationRef => Need::Interpretation,
    /// A property kind, well known or of the package.
    PropertyKindRef => Need::PropertyKind,
}

/// A qualified type the build knows — the list and its axes are said with
/// `#[param(need = Need::Type(…))]`, because they are values, not a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName(pub String);

impl Scalar for TypeName {
    fn need() -> Need {
        Need::Type(crate::types::TypeChoice::filtered(""))
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        <Self as Scalar>::need()
            .accepts(value)
            .map(|()| TypeName(value.as_str().unwrap_or_default().to_string()))
    }
    fn to_json(&self) -> Json {
        Json::from(self.0.as_str())
    }
}

/// A division of the BRGM chronostratigraphic chart — its label, its code or
/// its id, resolved by the operation against `energyml_domain::geolts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartTerm(pub String);

impl Scalar for ChartTerm {
    fn need() -> Need {
        Need::Vocabulary("geolts")
    }
    fn from_json(value: &Json) -> Result<Self, String> {
        match value {
            Json::String(text) => <Self as Scalar>::need()
                .accepts(value)
                .map(|()| ChartTerm(text.clone())),
            // An id typed as a number, or read back from an activity as a double.
            Json::Number(_) => whole(value)
                .map(|id| ChartTerm(id.to_string()))
                .ok_or_else(|| format!("expected a term of the chart, got {value}")),
            other => Err(format!("expected a term of the chart, got {other}")),
        }
    }
    fn to_json(&self) -> Json {
        Json::from(self.0.as_str())
    }
}

scalar_param!(
    f64,
    i64,
    usize,
    String,
    bool,
    Title,
    Epsg,
    ObjectRef,
    CrsRef,
    InterpretationRef,
    PropertyKindRef,
    TypeName,
    ChartTerm
);

/// The slot a field of type `T` declares, before its attributes.
pub fn slot_of<T: Param>(name: &'static str, about: &'static str) -> Slot {
    Slot {
        least: T::least(),
        most: T::most(),
        ..Slot::required(name, T::need(), about)
    }
}

/// A number's bounds and unit, from the field's attributes.
pub fn bounded(need: Need, min: Option<f64>, max: Option<f64>, uom: Option<&'static str>) -> Need {
    match need {
        Need::Number {
            uom: own,
            least,
            most,
        } => Need::Number {
            uom: uom.or(own),
            least: min.or(least),
            most: max.or(most),
        },
        Need::Integer { least, most } => Need::Integer {
            least: min.map(|v| v as i64).or(least),
            most: max.map(|v| v as i64).or(most),
        },
        other => other,
    }
}

/// A default, as the suggestion a form shows.
pub fn default_json<T: Param>(value: &T) -> Json {
    value.json().unwrap_or(Json::Null)
}

// ---------------------------------------------------------------- params

/// A block of parameters — a struct of fields, or a union of cases.
pub trait Params: Sized {
    /// The slots, in declaration order: what a form shows and a template
    /// projects. A controller always comes before what it controls.
    fn slots() -> Vec<Slot>;
    /// The block, from settled choices.
    fn read(choices: &crate::operation::Choices) -> Result<Self, Refusal>;
    /// The block **back** into choices — the inverse of [`Params::read`], for
    /// an operation that hands its settings to one still reading them by name
    /// (§6). An absent value is left out; a union writes its case's word.
    fn write(&self, choices: crate::operation::Choices) -> crate::operation::Choices;
}

/// No parameter at all.
impl Params for () {
    fn slots() -> Vec<Slot> {
        Vec::new()
    }
    fn read(_: &crate::operation::Choices) -> Result<Self, Refusal> {
        Ok(())
    }
    fn write(&self, choices: crate::operation::Choices) -> crate::operation::Choices {
        choices
    }
}

/// The slots of a union: its choice, then **each case's slots, acting only
/// under that case** — flattened on the wire, where a form already knows how
/// to render a slot with a condition (§5.3).
///
/// A slot two cases share (`radius`, for IDW and kriging) is **one** slot
/// whose condition names both; its kind must be the same in both, or the two
/// cases are asking two different things under one name. A slot already
/// conditioned inside its case — the sub-union of a case — keeps its own
/// condition: its controller is itself under the case, and the conditions
/// nest through it.
///
/// # Panics
///
/// On a declaration fault — two kinds under one name, or a case's slot that
/// is also `With`/`Without`, which a flat wire cannot combine with a case.
/// Every declaration is exercised by the template test, so a fault is a red
/// test, never a silent form.
pub fn union_slots(tag: Slot, cases: Vec<(&'static str, Vec<Slot>)>) -> Vec<Slot> {
    let tag_name = tag.name;
    let mut out = vec![tag];
    for (case, slots) in cases {
        for mut slot in slots {
            if let Some(existing) = out.iter_mut().find(|one| one.name == slot.name) {
                assert_eq!(
                    existing.need, slot.need,
                    "`{}` is declared by two cases of `{tag_name}` as two different things",
                    slot.name
                );
                match &mut existing.applies {
                    Applies::When { slot: controller, is }
                        if *controller == tag_name && !is.contains(&case) =>
                    {
                        is.push(case);
                    }
                    _ => {}
                }
                continue;
            }
            slot.applies = match slot.applies {
                Applies::Always => Applies::When {
                    slot: tag_name,
                    is: vec![case],
                },
                nested @ Applies::When { .. } => nested,
                other => panic!(
                    "`{}` is under the case `{case}` of `{tag_name}` and is also {other:?}: a flat form cannot combine the two",
                    slot.name
                ),
            };
            out.push(slot);
        }
    }
    out
}

// ---------------------------------------------------------------- inputs

/// A piece of the bag, with what is known of the object it came from.
#[derive(Debug, Clone, Copy)]
pub struct Piece<'a> {
    /// Its name in the bag.
    pub name: &'a str,
    /// What it is.
    pub source: &'a Source,
    /// What the caller knows of the object behind it, when it came from one.
    pub known: Option<&'a Known>,
}

/// The bag, each piece paired with what is known of it — **the one place the
/// pairing convention is written**.
///
/// A piece `X` is paired with the object piece `X (object)` — what
/// `read::pieces_of_object` and `read::in_slot` produce. And one convention
/// older than that, kept so that the callers written before it still bind: a
/// bag holding a single object piece named `object` and a single piece with
/// no pair gives the one to the other.
pub fn paired(bag: &Pieces) -> Vec<Piece<'_>> {
    let objects: Vec<(&str, &Known)> = bag
        .iter()
        .filter_map(|(name, source)| match source {
            Source::Object(known) => Some((name, known)),
            _ => None,
        })
        .collect();
    // **An object of the package is an input like any other — unless it is the
    // companion of another piece.**
    //
    // Widened because, in practice, one may want object-creation operations,
    // colour enrichment or problem detection that are not necessarily tied to
    // geometric objects. The mechanism used to bind only a piece **that has a
    // geometry**, so a `ColumnBasedTable`, a property kind or a colour map
    // could not be inputs — and `properties-from-table` could not continue an
    // existing table for that reason alone.
    //
    // The companion keeps its status: `X (object)` is *what is known of X*,
    // not an input of its own. Telling it apart by its **name** and not by its
    // nature is what lets the two coexist — it is the convention that
    // `read::pieces_of_object` writes, and it is read here and nowhere
    // else.
    let companion = |name: &str| -> bool { name.ends_with(" (object)") };
    let mut out: Vec<Piece<'_>> = bag
        .iter()
        .filter(|(name, source)| !matches!(source, Source::Object(_)) || !companion(name))
        .map(|(name, source)| Piece {
            name,
            source,
            known: match source {
                // An object piece **is** what is known of it: looking for a
                // companion named `X (object)` would be expecting a second one.
                Source::Object(known) => Some(known),
                _ => objects
                    .iter()
                    .find(|(object, _)| *object == format!("{name} (object)"))
                    .map(|(_, k)| *k),
            },
        })
        .collect();
    let legacy: Vec<&Known> = objects
        .iter()
        .filter(|(name, _)| *name == "object")
        .map(|(_, k)| *k)
        .collect();
    let unpaired: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, p)| p.known.is_none())
        .map(|(at, _)| at)
        .collect();
    if let ([known], [at]) = (legacy.as_slice(), unpaired.as_slice()) {
        out[*at].known = Some(known);
    }
    out
}

/// What is known of the object behind the piece a slot names — for an
/// operation's suggestions, which run on any bag, bound or not.
pub fn known_of<'a>(bag: &'a Pieces, slot: &str) -> Option<&'a Known> {
    let pieces = paired(bag);
    pieces
        .iter()
        .find(|piece| piece.name == slot)
        .or(match pieces.as_slice() {
            [only] => Some(only),
            _ => None,
        })
        .and_then(|piece| piece.known)
}

/// What is known of the piece of that name — an object piece itself, or the
/// object a piece is paired with. What a field's `over` resolves to: the
/// representation the values lie on, and how many nodes it has.
pub fn known_named<'a>(bag: &'a Pieces, name: &str) -> Option<&'a Known> {
    match bag.get(name) {
        Some(Source::Object(known)) => Some(known),
        Some(_) => paired(bag)
            .into_iter()
            .find(|piece| piece.name == name)
            .and_then(|piece| piece.known),
        None => None,
    }
}

/// One kind of input.
pub trait Input: Sized {
    /// The kind of piece — the vocabulary of [`Source::kind`].
    const KIND: &'static str;
    /// The input, from a piece of that kind.
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String>;
}

/// A field of an input struct: an input, `Option<T>` or `Vec<T>`.
pub trait InputParam: Sized {
    /// The input it holds.
    type Item: Input;
    /// How many at least.
    fn least() -> u32;
    /// How many at most.
    fn most() -> Option<u32>;
    /// The field, from the pieces bound to it — the count already checked.
    fn from_items(items: Vec<Self::Item>) -> Self;
}

macro_rules! input_param {
    ($($ty:ty),* $(,)?) => {$(
        impl InputParam for $ty {
            type Item = $ty;
            fn least() -> u32 { 1 }
            fn most() -> Option<u32> { Some(1) }
            fn from_items(mut items: Vec<$ty>) -> Self { items.remove(0) }
        }
        impl InputParam for Option<$ty> {
            type Item = $ty;
            fn least() -> u32 { 0 }
            fn most() -> Option<u32> { Some(1) }
            fn from_items(items: Vec<$ty>) -> Self { items.into_iter().next() }
        }
        impl InputParam for Vec<$ty> {
            type Item = $ty;
            fn least() -> u32 { 0 }
            fn most() -> Option<u32> { None }
            fn from_items(items: Vec<$ty>) -> Self { items }
        }
    )*};
}

/// A shape: geometry and topology — an object of the package, an artefact, a
/// mesh file.
#[derive(Debug, Clone)]
pub struct Shape {
    /// Its name in the bag.
    pub name: String,
    /// The patches.
    pub geometry: Geometry,
    /// The closure of its polylines, when the producer said.
    pub closed: Option<Vec<bool>>,
    /// What is known of the object it came from.
    pub known: Option<Known>,
}

impl Input for Shape {
    const KIND: &'static str = "shape";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Shape { geometry, closed } => Ok(Shape {
                name: piece.name.to_string(),
                geometry: geometry.clone(),
                closed: closed.clone(),
                known: piece.known.cloned(),
            }),
            other => Err(format!(
                "`{}` is {}, and a shape is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

/// Foreign features — a GeoJSON.
#[derive(Debug, Clone)]
pub struct Features {
    /// Its name in the bag.
    pub name: String,
    /// The file it came from, for a suggested stage or title.
    pub file: String,
    /// The features, in the producer's order.
    pub features: Vec<Feature>,
}

impl Input for Features {
    const KIND: &'static str = "features";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Features { name, features } => Ok(Features {
                name: piece.name.to_string(),
                file: name.clone(),
                features: features.clone(),
            }),
            other => Err(format!(
                "`{}` is {}, and features are wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

/// A table and its template.
#[derive(Debug, Clone)]
pub struct TableIn {
    /// Its name in the bag.
    pub name: String,
    /// The file it comes from.
    pub file: String,
    /// The rows.
    pub table: crate::tabular::Table,
    /// What its columns mean.
    pub template: crate::osdu::Template,
}

impl Input for TableIn {
    const KIND: &'static str = "table";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Table {
                name,
                table,
                template,
            } => Ok(TableIn {
                name: piece.name.to_string(),
                file: name.clone(),
                table: table.clone(),
                template: template.clone(),
            }),
            other => Err(format!(
                "`{}` is {}, and a table is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

/// An object of the package, taken as an input.
///
/// What an operation receives of it is what the caller has **resolved** — its
/// reference, its frame, and what `read::known_of_object` was able to read of
/// it. It therefore has nothing to look up, and `plan` stays pure: that is the
/// crate's rule, applied to an object that has no geometry.
#[derive(Debug, Clone)]
pub struct ObjectIn {
    /// Its name in the bag — the slot that received it.
    pub name: String,
    /// What is known of it.
    pub known: Known,
}

impl Input for ObjectIn {
    const KIND: &'static str = "object";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match (piece.source, piece.known) {
            (Source::Object(known), _) => Ok(ObjectIn {
                name: piece.name.to_string(),
                known: known.clone(),
            }),
            // **Having a geometry does not disqualify.** An object given to an
            // object slot arrives as **two** pieces when it is a
            // representation — the shape and its companion `X (object)`, which
            // `read::in_slot` writes — and it is the companion that is wanted.
            // Refusing on the nature of the first made an object slot
            // unreachable for everything that can be drawn: `wells-from-table`
            // takes frames, and a frame is a representation.
            //
            // The refusal is not lost, it is **in its place**: what an object
            // must carry to be useful is the operation's business, and only it
            // knows. `properties-from-table` already refuses by name a
            // `lookups` that holds no category, `wells-from-table` a
            // `wells` that states neither its head nor its depths.
            (_, Some(known)) => Ok(ObjectIn {
                name: piece.name.to_string(),
                known: known.clone(),
            }),
            (other, None) => Err(format!(
                "`{}` is {}, and an object of the package is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

/// A regular lattice of heights.
#[derive(Debug, Clone)]
pub struct LatticeIn {
    /// Its name in the bag.
    pub name: String,
    /// The lattice.
    pub lattice: Lattice,
    /// What is known of the object it came from.
    pub known: Option<Known>,
}

impl Input for LatticeIn {
    const KIND: &'static str = "lattice";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Lattice(lattice) => Ok(LatticeIn {
                name: piece.name.to_string(),
                lattice: lattice.clone(),
                known: piece.known.cloned(),
            }),
            other => Err(format!(
                "`{}` is {}, and a lattice is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

/// A field of values laid over another piece.
#[derive(Debug, Clone)]
pub struct Field {
    /// Its name in the bag.
    pub name: String,
    /// The name of the piece it lies on.
    pub over: String,
    /// `nodes`, `cells`, …
    pub on: String,
    /// One value per element.
    pub values: Vec<f64>,
    /// The unit, when known.
    pub uom: Option<String>,
}

impl Input for Field {
    const KIND: &'static str = "field";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Field {
                over,
                on,
                values,
                uom,
            } => Ok(Field {
                name: piece.name.to_string(),
                over: over.clone(),
                on: on.clone(),
                values: values.clone(),
                uom: uom.clone(),
            }),
            other => Err(format!(
                "`{}` is {}, and a field is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

/// A colour ramp.
#[derive(Debug, Clone)]
pub struct Ramp {
    /// Its name in the bag.
    pub name: String,
    /// Ascending.
    pub levels: Vec<f64>,
    /// One per band.
    pub colours: Vec<Hsv>,
}

impl Input for Ramp {
    const KIND: &'static str = "ramp";
    fn from_piece(piece: &Piece<'_>) -> Result<Self, String> {
        match piece.source {
            Source::Ramp { levels, colours } => Ok(Ramp {
                name: piece.name.to_string(),
                levels: levels.clone(),
                colours: colours.clone(),
            }),
            other => Err(format!(
                "`{}` is {}, and a ramp is wanted",
                piece.name,
                other.kind()
            )),
        }
    }
}

input_param!(Shape, Features, TableIn, ObjectIn, LatticeIn, Field, Ramp);

/// The input slot a field of type `T` declares.
pub fn input_slot_of<T: InputParam>(
    name: &'static str,
    about: &'static str,
    topology: Option<&'static str>,
    types: &'static [&'static str],
) -> Slot {
    let takes = Takes {
        kind: <T::Item as Input>::KIND,
        topology,
        types,
        palette: false,
    };
    Slot {
        least: T::least(),
        most: T::most(),
        ..Slot::required(name, Need::Input(takes), about)
    }
}

/// What an operation takes — a struct of inputs, bound by the framework.
pub trait Inputs: Sized {
    /// The input slots — what `takes()` answers.
    fn slots() -> Vec<Slot>;
    /// The inputs, bound from the bag, or the refusal that names the slot.
    fn bind(bag: &Pieces) -> Result<Self, Refusal>;
}

/// Nothing taken — `crs`.
impl Inputs for () {
    fn slots() -> Vec<Slot> {
        Vec::new()
    }
    fn bind(_: &Pieces) -> Result<Self, Refusal> {
        Ok(())
    }
}

/// **The one place that knows how pieces are named** (§5.5).
///
/// A slot `top` takes the piece `top`; a slot taking several takes `surfaces`,
/// `surfaces 2`, `surfaces 3`, in that order. A piece whose name is **no**
/// slot's goes to the **only required** slot of its kind — or, when no slot of
/// that kind is required, to the only slot of that kind. That is what a caller
/// that names nothing (the CLI without `slot=`, a test naming a piece by its
/// title) has always relied on: `interpolate` takes its samples unnamed and
/// its values (`field`) too, and its optional `faults` must be named. Two
/// candidate slots and an unnamed piece is a refusal that names them: guessing
/// which is the top is what reading roles by name exists to prevent.
pub struct Binder<'a> {
    pieces: Vec<Piece<'a>>,
    /// What the bag holds, in words — the other end of a refusal.
    kinds: String,
}

impl<'a> Binder<'a> {
    /// A binder over this bag.
    pub fn new(bag: &'a Pieces, _slots: &[Slot]) -> Self {
        Binder {
            pieces: paired(bag),
            kinds: bag.kinds(),
        }
    }

    /// The rank of a piece name under a slot name — `surfaces` 1,
    /// `surfaces 3` 3 — or `None` when it is not one of that slot's.
    fn rank(slot: &str, name: &str) -> Option<usize> {
        if name == slot {
            return Some(1);
        }
        let rest = name.strip_prefix(slot)?.strip_prefix(' ')?;
        rest.parse::<usize>().ok().filter(|n| *n >= 2)
    }

    /// The pieces of that kind that are no slot's by name.
    fn unnamed(&self, kind: &str, slots: &[Slot]) -> Vec<&Piece<'a>> {
        self.pieces
            .iter()
            .filter(|piece| piece.source.kind() == kind)
            .filter(|piece| !slots.iter().any(|one| Self::rank(one.name, piece.name).is_some()))
            .collect()
    }

    /// The slots an unnamed piece of that kind could go to: the required ones
    /// of its kind, or every one of its kind when none is required.
    fn rivals(kind: &str, slots: &[Slot]) -> Vec<&'static str> {
        let of_kind: Vec<&Slot> = slots
            .iter()
            .filter(|one| matches!(&one.need, Need::Input(takes) if takes.kind == kind))
            .collect();
        let required: Vec<&'static str> = of_kind
            .iter()
            .filter(|one| one.is_required())
            .map(|one| one.name)
            .collect();
        if required.is_empty() {
            of_kind.iter().map(|one| one.name).collect()
        } else {
            required
        }
    }

    /// Whether a piece of the bag goes to `slot` — named after it, or unnamed
    /// with `slot` its one candidate: the rule of [`Binder::take`], asked
    /// before anything is bound. See [`given_inputs`].
    pub fn receives(&self, slot: &Slot, slots: &[Slot]) -> bool {
        let Need::Input(takes) = &slot.need else {
            return false;
        };
        self.pieces
            .iter()
            .any(|piece| Self::rank(slot.name, piece.name).is_some())
            || (!self.unnamed(takes.kind, slots).is_empty() && Self::rivals(takes.kind, slots) == [slot.name])
    }

    /// The field bound to `slot`, checked against its declared count.
    pub fn take<P: InputParam>(&self, slot: &'static str, slots: &[Slot]) -> Result<P, Refusal> {
        let kind = <P::Item as Input>::KIND;
        let declared = slots.iter().find(|one| one.name == slot);
        let mut named: Vec<(usize, &Piece<'a>)> = self
            .pieces
            .iter()
            .filter_map(|piece| Self::rank(slot, piece.name).map(|rank| (rank, piece)))
            .collect();
        named.sort_by_key(|(rank, _)| *rank);
        let mut chosen: Vec<&Piece<'a>> = named.into_iter().map(|(_, piece)| piece).collect();

        // Pieces that are no slot's, of this kind — **appended** after the
        // named ones when this is their one candidate slot: a bag may name one
        // piece after the slot and bring others unnamed, and the writer takes
        // them all, as it always has.
        let unnamed = self.unnamed(kind, slots);
        if !unnamed.is_empty() {
            let rivals = Self::rivals(kind, slots);
            if rivals == [slot] {
                chosen.extend(unnamed);
            } else if chosen.is_empty() && declared.is_some_and(Slot::is_required) {
                return Err(Refusal::on(
                    slot,
                    format!(
                        "no piece is named `{slot}`, and {} unnamed {kind} piece(s) could be any of `{}` — name them after their slots",
                        unnamed.len(),
                        rivals.join("`, `")
                    ),
                ));
            }
        }

        let least = declared.map(|one| one.least).unwrap_or_else(P::least);
        let most = declared.map(|one| one.most).unwrap_or_else(P::most);
        let count = chosen.len() as u32;
        // **An empty `Every` slot is not yet a shortage**: the edge fills it
        // from the package (`read::every`) when it plans, and a list of offers
        // is judged before that. The operation says what an empty one means
        // when nothing filled it — `properties-from-table` refuses by its name.
        let filled_later =
            count == 0 && declared.is_some_and(|one| one.otherwise == crate::operation::Otherwise::Every);
        if count < least && !filled_later {
            let about = declared.map(|one| one.about).unwrap_or_default();
            return Err(Refusal::on(
                slot,
                // **Both ends**, the house rule: what it takes, and what the
                // bag holds instead — `nothing`, `shape`, `features`.
                match least {
                    1 => format!("it takes {about}; the input holds {}", self.kinds),
                    _ => format!("at least {least} piece(s) are needed, {count} given — {about}"),
                },
            ));
        }
        if let Some(most) = most {
            if count > most {
                // With what the slot says of itself: *at most one* is a count,
                // *one grid per plan* is the reason.
                let about = declared.map(|one| one.about).unwrap_or_default();
                return Err(Refusal::on(
                    slot,
                    format!("at most {most} piece(s), and {count} were given — {about}"),
                ));
            }
        }
        let items = chosen
            .into_iter()
            .map(|piece| <P::Item as Input>::from_piece(piece).map_err(|why| Refusal::on(slot, why)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(P::from_items(items))
    }
}

/// The input slots this bag gives a piece to — what [`Applies`] reads as
/// **given** beside the answered choices.
///
/// An input is not a choice: it arrives as a piece of the bag, never in the
/// answers. So a setting that is the alternative of an input — `spacing` and
/// `margin` of `interpolate`, which have no meaning once `onto` gives the
/// lattice — could not say `Without(["onto"])` and be heard: the condition
/// read the answers only, and `onto` is never among them. This is the one
/// place that answers *is that input given?*, by the naming rule of
/// [`Binder::take`], so that the condition and the binding cannot disagree.
pub fn given_inputs(bag: &Pieces, takes: &[Slot]) -> Vec<&'static str> {
    let binder = Binder::new(bag, takes);
    takes
        .iter()
        .filter(|slot| binder.receives(slot, takes))
        .map(|slot| slot.name)
        .collect()
}

// ---------------------------------------------------------------- suggestions

/// What an operation proposes from its inputs, posed on the declared slots
/// **by name** — and a name that is no slot's is kept, so that the adapter can
/// fail a test on it rather than drop a suggestion in silence.
pub struct Suggestions<'a> {
    slots: &'a mut Vec<Slot>,
    missed: Vec<String>,
}

impl<'a> Suggestions<'a> {
    /// Suggestions over these slots.
    pub fn new(slots: &'a mut Vec<Slot>) -> Self {
        Suggestions {
            slots,
            missed: Vec::new(),
        }
    }

    /// Propose `value` for `slot`.
    pub fn set(&mut self, slot: &str, value: Json) -> Option<&mut Slot> {
        match self.slots.iter_mut().position(|one| one.name == slot) {
            Some(at) => {
                let found = &mut self.slots[at];
                found.suggested = Some(value);
                Some(found)
            }
            None => {
                self.missed.push(slot.to_string());
                None
            }
        }
    }

    /// Propose `value` for `slot`, read off that object.
    pub fn inferred(&mut self, slot: &str, value: Json, from: Ref) {
        if let Some(found) = self.set(slot, value) {
            found.from = Some(crate::operation::Provenance::Input(from));
        }
    }

    /// A declared slot, to say more of it than a suggestion — what it creates
    /// when left out, when that depends on the operation.
    pub fn slot_mut(&mut self, slot: &str) -> Option<&mut Slot> {
        match self.slots.iter().position(|one| one.name == slot) {
            Some(at) => Some(&mut self.slots[at]),
            None => {
                self.missed.push(slot.to_string());
                None
            }
        }
    }

    /// Whether this operation declares that slot — for a suggestion shared by
    /// several operations, some of which leave a slot out on purpose
    /// (`boundary` makes its own interpretation, so it declares none).
    pub fn declared(&self, slot: &str) -> bool {
        self.slots.iter().any(|one| one.name == slot)
    }

    /// The names proposed for that are no slot's.
    pub fn missed(&self) -> &[String] {
        &self.missed
    }
}
