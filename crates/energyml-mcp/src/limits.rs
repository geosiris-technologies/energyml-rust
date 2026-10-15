//! The bounds of the server, and the rule that a call may lower them.
//!
//! They are settings of whoever runs the server (arguments, then
//! `ENERGYML_MCP_*` variables, read by the face that starts the process), never
//! a decision of a tool. A tool argument can ask for less than a bound, and
//! never for more: [`Limits::effective`] is the one place that says so.

use std::time::Duration;

use serde_json::{Value, json};

/// The bounds applied to every call. `Default` is the table of section 4.10 of
/// the design (decision Q5); built with [`Limits::default`] and the `with_*`
/// methods; `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// A hard bound on any serialised answer. Past it, the call is refused
    /// `too_large`; the list tools stop earlier, by their own byte budget, and
    /// say that they truncated.
    pub max_response_bytes: usize,
    /// The ceiling of the `limit` argument of every list.
    pub max_list: usize,
    /// How many object headers `energyml_find_resources` reads without a
    /// restricting `scope` or `type_contains`.
    pub max_header_scan: usize,
    /// How many array elements one call reads (the hard bound of a window).
    pub max_window_elements: usize,
    /// The ceiling of the `max_bytes` argument of `energyml_describe_object`.
    pub max_object_bytes: usize,
    /// How long a call may take. **Not enforced here**: this crate has no
    /// runtime. The adapter that owns the executor races each
    /// [`Registry::call`](crate::Registry::call) against this and answers with a
    /// [`RefusalCode::Timeout`](crate::RefusalCode::Timeout) refusal.
    pub call_timeout: Duration,
    /// Show the values of a dataspace's `custom_data` (OSDU access lists and
    /// legal tags among them) and not only its keys. Off: decision Q9.
    pub reveal_custom_data: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_response_bytes: 65_536,
            max_list: 500,
            max_header_scan: 2_000,
            max_window_elements: 10_000,
            max_object_bytes: 262_144,
            call_timeout: Duration::from_secs(30),
            reveal_custom_data: false,
        }
    }
}

impl Limits {
    /// With another response bound.
    pub fn with_max_response_bytes(mut self, bytes: usize) -> Self {
        self.max_response_bytes = bytes;
        self
    }

    /// With another list ceiling.
    pub fn with_max_list(mut self, items: usize) -> Self {
        self.max_list = items;
        self
    }

    /// With another header scan bound.
    pub fn with_max_header_scan(mut self, headers: usize) -> Self {
        self.max_header_scan = headers;
        self
    }

    /// With another window bound.
    pub fn with_max_window_elements(mut self, elements: usize) -> Self {
        self.max_window_elements = elements;
        self
    }

    /// With another object size ceiling.
    pub fn with_max_object_bytes(mut self, bytes: usize) -> Self {
        self.max_object_bytes = bytes;
        self
    }

    /// With another call timeout.
    pub fn with_call_timeout(mut self, timeout: Duration) -> Self {
        self.call_timeout = timeout;
        self
    }

    /// Showing (or hiding) the values of `custom_data`.
    pub fn with_reveal_custom_data(mut self, reveal: bool) -> Self {
        self.reveal_custom_data = reveal;
        self
    }

    /// What a call gets: the argument when it is given, the default otherwise,
    /// and **never more than the ceiling**. The argument may lower a bound; it
    /// cannot raise it.
    pub fn effective(requested: Option<usize>, default: usize, ceiling: usize) -> usize {
        requested.unwrap_or(default).min(ceiling)
    }

    /// The bytes a list may spend on its items: the response bound less a
    /// reserve for the rest of the answer.
    pub fn list_budget(&self) -> usize {
        self.max_response_bytes.saturating_sub(2_048)
    }

    /// The bounds in force, as `energyml_store_info` reports them.
    pub fn to_value(&self) -> Value {
        json!({
            "max_response_bytes": self.max_response_bytes,
            "max_list": self.max_list,
            "max_header_scan": self.max_header_scan,
            "max_window_elements": self.max_window_elements,
            "max_object_bytes": self.max_object_bytes,
            "call_timeout_seconds": self.call_timeout.as_secs(),
            "custom_data_values_shown": self.reveal_custom_data,
        })
    }
}
