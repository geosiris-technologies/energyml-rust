//! What is asked, and what a service declares it accepts.
//!
//! **No URL here.** A front end that built the address would be a second filter
//! encoder, and the day a provider does not speak the same language it would be
//! wrong without knowing it. Encoding belongs to the provider; this module only
//! holds the request.
//!
//! **Everything here crosses a boundary.** The declarations go to the front end, the
//! request comes back from it: both directions are serialised in `camelCase`,
//! the convention on the other side.

use serde::{Deserialize, Serialize};

/// A queryable collection, **declared** by its provider.
///
/// It is what the front end renders as a form. Nothing is guessed: a field the
/// service does not accept must not be offered, and a field it requires must be
/// visible before the button is pressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    /// The short name, the one that is typed: `boreholes`.
    pub name: String,
    /// The readable title.
    pub title: String,
    /// A sentence saying what is found in it.
    pub about: String,
    /// What the service calls this type: `gsmlbh:Borehole`.
    pub feature_type: String,
    /// The `energyml-build` operations a harvest can feed.
    ///
    /// Declared rather than guessed: it is what lets the front end offer *"create
    /// RESQML boreholes"* after a harvest, without knowing what a borehole is.
    pub feeds: Vec<String>,
    /// Whether an area can be passed, and under what name the service wants it.
    pub area: Option<AreaFilter>,
    /// The other filters, with their type and their suggestion.
    pub filters: Vec<Filter>,
}

/// How this service wants to be given an area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaFilter {
    /// The path of the geometric property, as the service expects it.
    ///
    /// For a complex-feature WFS this is not a column name but a path —
    /// `gsmlbh:referenceLocation/gsmlbh:OriginPosition/gsmlbh:location` — and
    /// nobody can guess it: it is declared.
    pub property: String,
    /// The label shown next to the drawing.
    pub label: String,
}

/// A filter a collection accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    /// The short name, the one that is typed: `codeSpace`.
    pub name: String,
    /// The label.
    pub label: String,
    /// What it means, in one sentence — **why** it is there.
    ///
    /// A label says *what*, this sentence says *what it is for*: without it,
    /// `codeSpace` is three words nobody knows how to fill in.
    pub about: String,
    /// The type of the value.
    pub kind: Kind,
    /// Mandatory? A required filter that is missing is a refusal **before** the
    /// request.
    pub required: bool,
    /// What the provider proposes, and which stays editable.
    ///
    /// A suggestion, never a lock — the rule of the creation door's `Slot`s.
    /// `None` when nothing sensible can be proposed: proposing a random value
    /// is worse than leaving it empty.
    pub suggest: Option<String>,
    /// The path the service expects, when it differs from the short name.
    pub property: String,
}

/// What a filter value can be.
///
/// Deliberately short. One more type is one more control to draw in the front end,
/// and a list that grows for no reason is a list that ends up rendered as a
/// text field anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Tagged, and the tag is beside it: `{"is":"text"}`, `{"is":"oneOf",
// "values":[…]}`. A front end that had to tell a string from an object to know which
// control to draw is a front end that will end up forgetting one.
#[serde(tag = "is", content = "values", rename_all = "camelCase")]
pub enum Kind {
    /// Free text.
    Text,
    /// A number.
    Number,
    /// A choice among these. The front end renders a drop-down list.
    OneOf(Vec<String>),
    /// True or false.
    Flag,
}

/// An area, **in WGS 84 degrees**.
///
/// The format of the map view, that of GeoJSON (RFC 7946), and the one a drawn
/// outline already gives. Converting elsewhere would be a second geodesy; the
/// axis order a service expects is the business of that service and of nobody
/// else — see [`crate::wfs`], which measured it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "as", content = "of", rename_all = "camelCase")]
pub enum Area {
    /// A box: west, south, east, north.
    Box {
        /// Minimum longitude.
        west: f64,
        /// Minimum latitude.
        south: f64,
        /// Maximum longitude.
        east: f64,
        /// Maximum latitude.
        north: f64,
    },
    /// A closed ring, longitude then latitude, the first point repeated at the
    /// end — what the map view writes.
    Ring(Vec<(f64, f64)>),
}

impl Area {
    /// The extent of a ring, for a service that only takes a box.
    ///
    /// **Widening is not lying, provided it is said**: the box contains the
    /// ring, so it returns *more* entities, never fewer. A caller who wants
    /// exactly the ring must filter what comes back, and the provider says
    /// which of the two it did.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        match self {
            Area::Box {
                west,
                south,
                east,
                north,
            } => (*west, *south, *east, *north),
            Area::Ring(points) => points.iter().fold(
                (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                |(w, s, e, n), (lon, lat)| (w.min(*lon), s.min(*lat), e.max(*lon), n.max(*lat)),
            ),
        }
    }

    /// The number of vertices, to say what is about to be sent.
    pub fn corners(&self) -> usize {
        match self {
            Area::Box { .. } => 4,
            Area::Ring(points) => points.len(),
        }
    }
}

/// A condition on a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Term {
    /// The short name of the filter, as the collection declares it.
    pub name: String,
    /// The value, as it was typed.
    pub value: String,
}

/// What somebody asks for.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    /// The collection, by its short name.
    pub collection: String,
    /// Where, in degrees. `None` means *everywhere*, which is rarely wanted and
    /// never assumed: it is for the caller to say so.
    pub area: Option<Area>,
    /// The conditions, all conjoined.
    ///
    /// Conjoined and not a tree: it can be drawn as a form, read aloud, and
    /// there is no precedence to learn — the same argument the scene's
    /// `Selector` already settled.
    pub terms: Vec<Term>,
    /// The ceiling the caller sets, in **entities**.
    ///
    /// In entities and not in bytes, because that is what somebody knows of
    /// their request. A ceiling in bytes exists too, but it belongs to the
    /// transport: they are two protections against two different accidents.
    pub most: Option<usize>,
}

/// A page to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// The rank of the first wanted entity.
    pub start: usize,
    /// How many to ask for.
    pub count: usize,
}

impl Page {
    /// The first page of `count` entities.
    pub fn first(count: usize) -> Self {
        Page { start: 0, count }
    }

    /// The next one, of the same size.
    pub fn next(self) -> Self {
        Page {
            start: self.start + self.count,
            count: self.count,
        }
    }
}
