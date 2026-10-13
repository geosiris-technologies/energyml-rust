//! The BSS boreholes service, as it was measured.
//!
//! **This file only holds what a generic WFS cannot guess**: the address, the
//! two collections, the paths its filters expect, and the axis order it
//! imposes. Everything else is in [`crate::wfs`] - which is what makes the next
//! service nearly free.
//!
//! Everything here was read off the service itself (`GetCapabilities` and
//! requests), for lack of public documentation; the figures are given as
//! measured at the time and will age.

use crate::query::{AreaFilter, Collection, Filter, Kind};
use crate::wfs::{Axes, Wfs};

/// The address of the service.
pub const ENDPOINT: &str = "https://data.geoscience.fr/api/gsmlbh/ows";

/// The BSS boreholes service, in GeoSciML 4.1.
pub fn bss() -> Wfs {
    Wfs {
        name: "brgm-bss".to_string(),
        about: "The boreholes of the Banque du sous-sol (BRGM), as GeoSciML 4.1 Borehole".to_string(),
        endpoint: ENDPOINT.to_string(),
        attribution: "BRGM - Banque du sous-sol (BSS)".to_string(),
        // **`None` does not mean "free".** The service's capabilities declare
        // neither fees nor constraints (`ows:Fees` and `ows:AccessConstraints`
        // are empty), which is not the same as a stated licence. As long as
        // nobody has confirmed one, displaying one would invent a credit.
        licence: None,
        crs: "urn:ogc:def:crs:EPSG::4326".to_string(),
        // **Measured, and not the way it was believed.** The same service wants
        // `lat lon` in the `bbox` parameter with the URN form of EPSG:4326, and
        // `lon lat` in a CQL `SRID=4326;POLYGON(...)`. Compared on the same
        // area: 16,984 entities one way, **0 the other**. A convention
        // generalised from one parameter to the other is a request that answers
        // "nothing here" without signalling anything.
        axes: Axes::LonLat,
        collections: vec![boreholes(), intervals()],
    }
}

/// The boreholes, with their nested intervals.
fn boreholes() -> Collection {
    Collection {
        name: "boreholes".to_string(),
        title: "Boreholes".to_string(),
        about: "A borehole: its head, its ground level, its measured depths and the \
                intervals the logs describe."
            .to_string(),
        feature_type: "gsmlbh:Borehole".to_string(),
        // What `energyml-build` can make of it. Declared here rather than
        // guessed elsewhere: it is what lets the front end offer the next step
        // without knowing what a borehole is.
        feeds: vec!["sigeol-boreholes".to_string()],
        area: Some(AreaFilter {
            // Nobody guesses this path: it is the geometry *of the origin
            // position*, not that of the entity.
            property: "gsmlbh:referenceLocation/gsmlbh:OriginPosition/gsmlbh:location".to_string(),
            label: "Area".to_string(),
        }),
        filters: vec![Filter {
            name: "codeSpace".to_string(),
            label: "Interval producer".to_string(),
            about: "Keep only the boreholes whose intervals come from this producer. \
                    Without this filter, most boreholes come back **with no interval at all**: \
                    measured on an area around Paris, 16,984 boreholes in all and 2,940 with \
                    BRGM intervals."
                .to_string(),
            kind: Kind::OneOf(vec!["BRGM".to_string()]),
            required: false,
            // Suggested because it is the useful answer in almost every case,
            // and editable because it is a suggestion.
            suggest: Some("BRGM".to_string()),
            property: "gsmlbh:logElement/gsmlbh:BoreholeInterval/gsmlbh:collectionIdentifier/@codeSpace"
                .to_string(),
        }],
    }
}

/// The intervals alone - 2,303,818 entities, as measured.
fn intervals() -> Collection {
    Collection {
        name: "intervals".to_string(),
        title: "Borehole intervals".to_string(),
        about: "A slice of borehole and what it crosses. The same fact as \
                `boreholes` from the other end: here the intervals are the entities, \
                and each one names its parent borehole."
            .to_string(),
        feature_type: "gsmlbh:BoreholeInterval".to_string(),
        // Nothing consumes them today: an interval alone is not a well, and
        // pretending otherwise would write objects without a head.
        feeds: Vec::new(),
        // **No area.** A `BoreholeInterval` carries no geometry of its own
        // (measured: an empty `GeometryCollection`); offering to draw a polygon
        // here would offer a gesture with no effect.
        area: None,
        filters: vec![Filter {
            name: "codeSpace".to_string(),
            label: "Producer".to_string(),
            about: "Keep only the intervals of this producer.".to_string(),
            kind: Kind::OneOf(vec!["BRGM".to_string()]),
            required: false,
            suggest: Some("BRGM".to_string()),
            property: "gsmlbh:collectionIdentifier/@codeSpace".to_string(),
        }],
    }
}
