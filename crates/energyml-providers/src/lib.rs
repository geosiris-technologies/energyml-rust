//! Querying public APIs, and reading what they answer.
//!
//! This module holds the boundary around which everything else is built:
//!
//! > **Building a request and reading an answer are pure. The round trip is
//! > not.**
//!
//! What is pure can be tested without a network — and most of what can be wrong
//! is there: the encoding of a filter, the axis order of an area, the reading of
//! a server refusal. The round trip is a [`Transport`], a trait, with a native
//! implementation behind the `native` feature; in the browser it is the front end's
//! `fetch`, and nothing of this crate is used there.
//!
//! # What a provider declares, and why the front end depends on it
//!
//! A [`Provider`] does not merely answer: it **declares** what it offers — its
//! [`Collection`]s, and for each one the [`Filter`]s it accepts, with their
//! type, their obligation and a suggested value. That is what lets the front end draw
//! a form **without spelling a single field name**, exactly as it renders the
//! `Slot`s of an operation: a provider added here appears there without a line
//! changing.
//!
//! The crate's source and issues are at
//! <https://github.com/geosiris-technologies/energyml-rust>.

#![warn(missing_docs)]

pub mod error;
pub mod query;
pub mod registry;
pub mod transport;
pub mod wfs;

pub mod brgm;
pub mod ign_altimetrie;
pub mod terrain;

pub use error::{Error, Result};
pub use query::{Area, Collection, Filter, Kind, Page, Query, Term};
pub use registry::{Catalogue, Declared, Providers};
pub use transport::{Answer, Request, Transport};

use energyml_build::geojson::{Feature, Flow, Summary, Wanted};

/// A queryable service, and what it offers.
///
/// A provider is **not a server**: it is a service *and* what it exposes. Two
/// collections of the same host may not speak the same dialect — measured on
/// `data.geoscience.fr`, whose boreholes' `GeologicUnit` is not that of the 3D
/// model.
pub trait Provider {
    /// The short name, the one that is typed: `brgm-bss`.
    fn name(&self) -> &str;

    /// One sentence, for a list.
    fn describe(&self) -> &str;

    /// What the service says of itself: its address, and whom to credit.
    fn about(&self) -> About;

    /// What it offers, **declared**. The front end makes a form of it.
    fn collections(&self) -> Vec<Collection>;

    /// The quote: the request that **counts without bringing back**.
    ///
    /// A request like any other, not a mode: a quote that took another path than
    /// the harvest would not tell the price of the harvest.
    fn count(&self, query: &Query) -> Result<Request>;

    /// A page of results.
    fn page(&self, query: &Query, page: Page) -> Result<Request>;

    /// Read a count. **Pure**, and it consumes the answer: reading a body is
    /// consuming it, and pretending otherwise would suggest it can be read
    /// again.
    fn read_count(&self, answer: Answer) -> Result<u64>;

    /// Read a page, entity by entity. **Pure**, and streamed.
    ///
    /// The `sink` decides whether to continue or stop; nothing of the document
    /// is held, neither the text nor a JSON tree — see `read_streaming`.
    fn read_page(
        &self,
        answer: Answer,
        wanted: &Wanted<'_>,
        sink: &mut dyn FnMut(Feature) -> Flow,
    ) -> Result<Summary>;
}

/// What a service says of itself.
///
/// The address **and** the credit come together, a rule the basemap has already
/// paid for: two reads from two places end up showing one provider's data under
/// another's name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// The address of the service.
    pub endpoint: String,
    /// Who publishes it, as it must be written on screen.
    pub attribution: String,
    /// Its licence, when known — `None` does not mean "free".
    pub licence: Option<String>,
}
