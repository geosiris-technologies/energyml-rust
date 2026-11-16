//! Which properties are siblings of one another: the one rule.
//!
//! Two properties of the same support and element are *siblings* when they
//! differ by a direction (`Rh` and `Rv`, `kx`, `ky`, `kz`), by a time (the
//! annual steps of a series of properties), or when one is a radius and the
//! other its angles (the polar pair of a dip log). This file is **the** rule:
//! [`kinship`] is a pure function of two [`KinFacts`], and the two callers
//! feed it what they have, never a second implementation of the comparison:
//!
//! - `PackageIndex::siblings` builds the facts from the sheets of the index
//!   ([`KinFacts::of_sheet`]) and reads no object;
//! - a check that has no index builds them from properties it has read
//!   ([`KinFacts::of_property`]).
//!
//! # What the rule does not know
//!
//! The kind is compared **as written**: the uuid of a kind object, or the name
//! of a RESQML 2.0.1 standard kind. Neither caller has a kind resolver, so two
//! different kind objects of the same quantity class are not siblings here
//! (RESQML TRG 2.2 does not say they would be; resolving them is the
//! `property_kind` module's job, and it would need the package).
//!
//! The time of a 2.0.1 property is its `TimeIndex.Index`, compared as a
//! number: two indices of two different time series would read as two steps.
//! A package with several series on the same support is not told apart here.
//!
//! # Sources
//!
//! - EML TRG v2.3 §11.11 `PropertyKindFacet` p. 294: one facet of a kind per
//!   property, so "the `direction` facet" is unambiguous.
//! - RESQML TRG v2.2 §7.1 `AbstractProperty` p. 222: the value count per
//!   element, the same for every element of a property.

use energyml_core::identity;

use crate::package_index::{PropertySheet, SheetKind};
use crate::property::{Facet, Instant, Property, PropertyKindRef};
use crate::units::Dimension;

/// When a property was taken.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum TimeKey {
    /// 2.0.1 `TimeIndex.Index` into the series.
    Index(u64),
    /// 2.2 `Time`.
    Instant(Instant),
}

/// How two properties are related.
#[derive(Clone, PartialEq, Debug)]
pub enum Kinship {
    /// Same kind, unit, support, element and shape; their `direction` facets differ.
    Direction {
        /// The `direction` facet of the first property.
        this: Facet,
        /// The `direction` facet of the other.
        other: Facet,
    },
    /// Same kind, unit, support, element and shape; taken at different times.
    TimeStep {
        /// When the first property was taken.
        this: TimeKey,
        /// When the other was.
        other: TimeKey,
    },
    /// One length and one angle, both `[k]`, `k >= 3`, on the same support and element.
    PolarPair {
        /// Uri of the property in a unit of length.
        radius: String,
        /// Uri of the property in a unit of plane angle.
        angles: String,
    },
    /// Same kind, unit, support, element and shape, and nothing told apart.
    SameKind,
}

/// A sibling of a property, as `PackageIndex::siblings` lists it.
#[derive(Clone, PartialEq, Debug)]
pub struct Sibling {
    /// Full uri of the sibling.
    pub uri: String,
    /// How it relates to the property asked about.
    pub kinship: Kinship,
}

/// What the kinship rule compares of one property: facts a `PropertySheet` and
/// a read `Property` both carry.
#[derive(Clone, PartialEq, Debug)]
pub struct KinFacts<'a> {
    /// Full uri of the property.
    pub uri: &'a str,
    /// The supporting representation, as the property names it: a full uri or
    /// a uuid, the rule reads the uuid of either (`identity::parse_address`).
    pub support: &'a str,
    /// The indexable element, as written (`nodes`, `intervals`...).
    pub indexable: &'a str,
    /// The kind as written, no resolver (the index has none): the uuid of a
    /// kind object, or the name of a RESQML 2.0.1 standard kind - what
    /// `SheetKind` and `PropertyKindRef` both say.
    pub kind: Option<&'a str>,
    /// The unit, when the property has one.
    pub uom: Option<&'a str>,
    /// Values per element, slowest dimension first (empty is a scalar).
    pub value_count_per_element: &'a [u64],
    /// Qualifiers.
    pub facets: &'a [Facet],
    /// When the property was taken, when it says.
    pub time: Option<TimeKey>,
}

impl<'a> KinFacts<'a> {
    /// The facts of a sheet. `None` when the sheet names no supporting
    /// representation: a property with no support has no siblings.
    pub fn of_sheet(uri: &'a str, sheet: &'a PropertySheet) -> Option<KinFacts<'a>> {
        let support = sheet.supporting_representation.as_deref()?;
        let kind = sheet.kind.as_ref().map(|kind| match kind {
            SheetKind::Standard(name) => &**name,
            SheetKind::Object { uuid, .. } => &**uuid,
        });
        let time = sheet
            .time
            .as_deref()
            .and_then(|time| time.index)
            .map(TimeKey::Index)
            .or_else(|| sheet.instant.as_deref().cloned().map(TimeKey::Instant));
        Some(KinFacts {
            uri,
            support,
            indexable: sheet.indexable.as_str(),
            kind,
            uom: sheet.uom.as_deref(),
            value_count_per_element: &sheet.value_count_per_element,
            facets: &sheet.facets,
            time,
        })
    }

    /// The facts of a property read by [`property::read`](crate::property::read).
    /// `uri` and `support` are full uris: a `Property` carries uuids and the
    /// caller, who has the package, knows the identities.
    pub fn of_property(uri: &'a str, support: &'a str, property: &'a Property<'_>) -> KinFacts<'a> {
        let kind = property.kind.as_ref().map(|kind| match kind {
            PropertyKindRef::Standard { name } => name.as_str(),
            PropertyKindRef::Object { uuid, .. } => uuid.as_str(),
        });
        let time = property
            .time
            .as_ref()
            .and_then(|time| time.index)
            .map(TimeKey::Index)
            .or_else(|| property.instant.clone().map(TimeKey::Instant));
        KinFacts {
            uri,
            support,
            indexable: property.indexable.as_str(),
            kind,
            uom: property.uom.as_deref(),
            value_count_per_element: &property.value_count_per_element,
            facets: &property.facets,
            time,
        }
    }
}

/// How `b` relates to `a`: the one rule. `None` when they are not siblings
/// (another support, another element, another shape or kind, or the same
/// property).
///
/// 1. The same support and the same element, else nothing.
/// 2. A radius and its angles (`[k]`, `k >= 3`, one length and one angle, the
///    kinds free) are a [`Kinship::PolarPair`].
/// 3. Otherwise the same kind, the same unit and the same shape are required,
///    and then, in this order: two different `direction` facets, two different
///    times, else [`Kinship::SameKind`].
pub fn kinship(a: &KinFacts<'_>, b: &KinFacts<'_>) -> Option<Kinship> {
    if a.uri.eq_ignore_ascii_case(b.uri) {
        return None;
    }
    if identity::parse_address(a.support).0 != identity::parse_address(b.support).0 {
        return None;
    }
    if !a.indexable.eq_ignore_ascii_case(b.indexable) {
        return None;
    }
    let (shape_a, shape_b) = (shape_of(a), shape_of(b));
    if let Some(pair) = polar_pair(a, b, shape_a, shape_b) {
        return Some(pair);
    }
    let (kind_a, kind_b) = (a.kind?, b.kind?);
    if !kind_a.eq_ignore_ascii_case(kind_b) || a.uom != b.uom || shape_a != shape_b {
        return None;
    }
    if let (Some(this), Some(other)) = (direction_of(a.facets), direction_of(b.facets))
        && this.value.as_str() != other.value.as_str()
    {
        return Some(Kinship::Direction {
            this: this.clone(),
            other: other.clone(),
        });
    }
    if let (Some(this), Some(other)) = (&a.time, &b.time)
        && this != other
    {
        return Some(Kinship::TimeStep {
            this: this.clone(),
            other: other.clone(),
        });
    }
    Some(Kinship::SameKind)
}

/// The shape, with the empty counts of a scalar read as `[1]`.
fn shape_of<'a>(facts: &KinFacts<'a>) -> &'a [u64] {
    if facts.value_count_per_element.is_empty() {
        &[1]
    } else {
        facts.value_count_per_element
    }
}

/// The only `direction` facet, when there is exactly one.
fn direction_of(facets: &[Facet]) -> Option<&Facet> {
    let mut found = facets
        .iter()
        .filter(|facet| facet.kind.eq_ignore_ascii_case("direction"));
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

fn dimension_of(facts: &KinFacts<'_>) -> Option<Dimension> {
    facts.uom.and_then(Dimension::of)
}

/// One length and one angle, both `[k]` with `k >= 3`.
fn polar_pair(a: &KinFacts<'_>, b: &KinFacts<'_>, shape_a: &[u64], shape_b: &[u64]) -> Option<Kinship> {
    if shape_a != shape_b || shape_a.len() != 1 || shape_a[0] < 3 {
        return None;
    }
    match (dimension_of(a)?, dimension_of(b)?) {
        (Dimension::Length, Dimension::PlaneAngle) => Some(Kinship::PolarPair {
            radius: a.uri.to_string(),
            angles: b.uri.to_string(),
        }),
        (Dimension::PlaneAngle, Dimension::Length) => Some(Kinship::PolarPair {
            radius: b.uri.to_string(),
            angles: a.uri.to_string(),
        }),
        _ => None,
    }
}
