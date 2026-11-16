//! What the two conformance harnesses ([`crate::contract`] and
//! [`crate::contract_witsml`]) share: reading a uri into the uuid or dataspace
//! the rules talk about, and showing sets of uuids with the harness's own labels.
//!
//! Each harness keeps its table of labels (its own fixtures); the helpers that
//! do not depend on the table are here once. The uri readings go through the
//! one door, [`EtpUri`], and the dataspace is spelled by it too.

use std::collections::BTreeSet;

use energyml_core::identifiers::EtpUri;

use crate::context::Found;
use crate::graph::Edge;

/// The dataspace uri a uri lives in.
pub(crate) fn dataspace_of(uri: &str) -> String {
    EtpUri::dataspace_uri_of(uri)
}

/// The uuid (lower case) a uri names; the text itself when it names none.
pub(crate) fn uuid_of(uri: &str) -> String {
    EtpUri::uuid_of(uri)
        .unwrap_or_else(|| uri.to_string())
        .to_ascii_lowercase()
}

/// A set of uuids, by the harness's labels.
pub(crate) fn show(uuids: &BTreeSet<String>, label: fn(&str) -> String) -> String {
    format!(
        "{{{}}}",
        uuids
            .iter()
            .map(|uuid| label(uuid))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A set of `from -> to` pairs, by the harness's labels.
pub(crate) fn show_pairs(pairs: &BTreeSet<(String, String)>, label: fn(&str) -> String) -> String {
    format!(
        "{{{}}}",
        pairs
            .iter()
            .map(|(from, to)| format!("{} -> {}", label(from), label(to)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The uuids as a lower-case set.
pub(crate) fn set(uuids: &[&str]) -> BTreeSet<String> {
    uuids.iter().map(|uuid| uuid.to_ascii_lowercase()).collect()
}

/// The uuids of what a read found.
pub(crate) fn uuids_of(found: &Found) -> BTreeSet<String> {
    found
        .resources
        .iter()
        .map(|summary| summary.uuid.to_ascii_lowercase())
        .collect()
}

/// The `(from, to)` uuids of edges.
pub(crate) fn pairs(edges: &[Edge]) -> BTreeSet<(String, String)> {
    edges
        .iter()
        .map(|edge| (uuid_of(&edge.from), uuid_of(&edge.to)))
        .collect()
}
