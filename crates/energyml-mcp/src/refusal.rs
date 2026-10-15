//! A refusal is an answer: the tool says what it will not do, and why.
//!
//! The rule of the repository is *refuse rather than draw*: an empty list that
//! means "this store cannot" looks exactly like a store with nothing in it. So a
//! tool that cannot answer returns a [`Refusal`], which the MCP layer turns into
//! a tool execution error (`isError: true`) the model can read and act on, and
//! never an empty result.
//!
//! # Untrusted text
//!
//! Titles, server messages and file names come from the store and may carry
//! instructions aimed at the model. A [`Refusal`] therefore keeps its
//! [`message`](Refusal::message) a fixed sentence written here, and puts
//! whatever the store said in [`detail`](Refusal::detail) and
//! [`candidates`](Refusal::candidates), which are data fields. Secrets are
//! removed from `detail` on the way in ([`redact`]).

use std::fmt;

use energyml_storage::error::{Error, ExternalDataReason};
use serde_json::{Map, Value, json};

/// Why a tool refused. The word the model sees is [`RefusalCode::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RefusalCode {
    /// No such object, dataspace or array.
    NotFound,
    /// An identifier names more than one object.
    Ambiguous,
    /// A URI the store cannot use, or that does not parse.
    BadUri,
    /// The store does not offer this operation. Not retryable.
    Unsupported,
    /// A capability is missing (for instance HDF5 not compiled in): the data is
    /// not absent, the means to read it is.
    FeatureNotEnabled,
    /// The data lives in a file or a path that cannot be reached.
    ExternalData,
    /// The answer would exceed a bound of the server.
    TooLarge,
    /// The remote server failed or refused the request as a whole.
    Remote,
    /// An argument is missing, unknown, of the wrong type or out of range.
    BadArguments,
    /// The call took longer than the server allows. Produced by the adapter
    /// that owns the runtime ([`crate::Limits::call_timeout`]), never by a tool.
    Timeout,
    /// Anything else, including an [`Error`] variant this crate does not know.
    Other,
}

impl RefusalCode {
    /// The snake_case word of the MCP answer.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::NotFound => "not_found",
            RefusalCode::Ambiguous => "ambiguous",
            RefusalCode::BadUri => "bad_uri",
            RefusalCode::Unsupported => "unsupported",
            RefusalCode::FeatureNotEnabled => "feature_not_enabled",
            RefusalCode::ExternalData => "external_data",
            RefusalCode::TooLarge => "too_large",
            RefusalCode::Remote => "remote",
            RefusalCode::BadArguments => "bad_arguments",
            RefusalCode::Timeout => "timeout",
            RefusalCode::Other => "other",
        }
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A tool declining to answer, in terms the model can act on.
///
/// Serialises to `{"refusal": {...}}` ([`Refusal::to_value`]). Built with
/// [`Refusal::new`] and the `with_*` methods; `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    /// The reason, as a code.
    pub code: RefusalCode,
    /// A fixed sentence written by this crate. Never contains store text.
    pub message: String,
    /// The kind of store that refused (`EPC`, `etp`), when a store did.
    pub store: Option<String>,
    /// What was attempted, when the store named it.
    pub operation: Option<String>,
    /// What the model can do next.
    pub hint: Option<String>,
    /// What the store or the caller said, verbatim and with secrets removed.
    /// Data, not a sentence of the server and not an instruction.
    pub detail: Option<String>,
    /// For [`RefusalCode::Ambiguous`]: every object the identifier could mean,
    /// each by its full URI. Empty otherwise.
    pub candidates: Vec<String>,
}

impl Refusal {
    /// A refusal with a code and a fixed sentence.
    pub fn new(code: RefusalCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            store: None,
            operation: None,
            hint: None,
            detail: None,
            candidates: Vec::new(),
        }
    }

    /// An argument is wrong. `message` names the argument and the rule.
    pub fn bad_arguments(message: impl Into<String>) -> Self {
        Self::new(RefusalCode::BadArguments, message)
    }

    /// The answer would be larger than the server allows.
    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(RefusalCode::TooLarge, message)
    }

    /// With the store that refused.
    pub fn with_store(mut self, store: impl Into<String>) -> Self {
        self.store = Some(store.into());
        self
    }

    /// With the operation that was attempted.
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// With advice for the model.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// With what was said, verbatim. Secrets are removed ([`redact`]).
    pub fn with_detail(mut self, detail: impl AsRef<str>) -> Self {
        self.detail = Some(redact(detail.as_ref()));
        self
    }

    /// With the candidates of an ambiguous identifier.
    pub fn with_candidates(mut self, candidates: Vec<String>) -> Self {
        self.candidates = candidates;
        self
    }

    /// The MCP answer: `{"refusal": {"code", "message", ...}}`, absent fields
    /// left out.
    pub fn to_value(&self) -> Value {
        let mut body = Map::new();
        body.insert("code".into(), json!(self.code.as_str()));
        body.insert("message".into(), json!(self.message));
        if let Some(store) = &self.store {
            body.insert("store".into(), json!(store));
        }
        if let Some(operation) = &self.operation {
            body.insert("operation".into(), json!(operation));
        }
        if let Some(hint) = &self.hint {
            body.insert("hint".into(), json!(hint));
        }
        if let Some(detail) = &self.detail {
            body.insert("detail".into(), json!(detail));
        }
        if !self.candidates.is_empty() {
            body.insert("candidates".into(), json!(self.candidates));
        }
        json!({ "refusal": Value::Object(body) })
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Refusal {}

/// The mapping of section 4.0 of the design: what a store said, as a refusal.
///
/// `Error` and `ExternalDataReason` are `#[non_exhaustive]`: the `_` arms turn
/// a variant added later into [`RefusalCode::Other`] rather than a build
/// failure of every face, and a test per reachable row guards the rest.
impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        match error {
            Error::NotFound(uri) => Refusal::new(RefusalCode::NotFound, "no object at the given URI")
                .with_hint("list first (energyml_list_resources or energyml_find_resources) and copy a URI as returned")
                .with_detail(uri),
            Error::Ambiguous { uri, candidates } => Refusal::new(
                RefusalCode::Ambiguous,
                "the identifier names more than one object",
            )
            .with_hint("choose one of the candidates and pass its full URI")
            .with_detail(uri)
            .with_candidates(candidates),
            Error::BadUri { uri, reason } => Refusal::new(
                RefusalCode::BadUri,
                "the URI cannot be used here",
            )
            .with_hint("copy the URI exactly as another tool returned it")
            .with_detail(format!("{uri}: {reason}")),
            Error::OutOfBounds { location, reason } => {
                Refusal::new(RefusalCode::BadArguments, "the request reaches outside what it asks into")
                    .with_hint("see `dimensions` in energyml_list_arrays")
                    .with_detail(redact(&format!("{location}: {reason}")))
            }
            Error::TooLarge { location, reason } => {
                Refusal::new(RefusalCode::TooLarge, "the request is larger than the bound")
                    .with_hint("narrow `count`, or ask for statistics only")
                    .with_detail(redact(&format!("{location}: {reason}")))
            }
            Error::UnknownType(qualified_type) => Refusal::new(
                RefusalCode::Unsupported,
                "this server does not know the type of the object",
            )
            .with_hint("the schema version is not registered on this server; do not retry")
            .with_detail(qualified_type),
            Error::Unsupported { operation, store } => {
                Refusal::new(RefusalCode::Unsupported, "the store does not offer this operation")
                    .with_store(store)
                    .with_operation(operation)
                    .with_hint("this store cannot do it; do not retry, and say so")
            }
            Error::ExternalData { location, reason } => match reason {
                ExternalDataReason::FeatureNotEnabled { feature, mime } => Refusal::new(
                    RefusalCode::FeatureNotEnabled,
                    "a capability is missing on this server; the data is not absent",
                )
                .with_hint(format!("the server was built without the `{feature}` feature"))
                .with_detail(match mime {
                    Some(mime) => format!("{location} ({mime})"),
                    None => location,
                }),
                other => Refusal::new(RefusalCode::ExternalData, "the external data cannot be read")
                    .with_hint("say what is missing; the object itself is fine")
                    .with_detail(format!("{location}: {other}")),
            },
            Error::Remote { store, reason } => {
                Refusal::new(RefusalCode::Remote, "the remote server failed or refused the request")
                    .with_store(store)
                    .with_hint("retry later; say that the server refused")
                    .with_detail(reason)
            }
            // The system error text may carry a path of the host: not repeated.
            Error::Io(_) => Refusal::new(RefusalCode::Other, "reading the underlying data failed"),
            Error::Package(reason) => {
                Refusal::new(RefusalCode::Other, "the package is malformed").with_detail(reason)
            }
            Error::Xml(reason) => {
                Refusal::new(RefusalCode::Other, "the XML of an object could not be read").with_detail(reason)
            }
            Error::Transaction { store, reason } => {
                Refusal::new(RefusalCode::Other, "a transaction was misused")
                    .with_store(store)
                    .with_detail(reason)
            }
            Error::Core(inner) => {
                Refusal::new(RefusalCode::Other, "the object could not be read").with_detail(inner.to_string())
            }
            // Variants added to the storage `Error` later land here.
            _ => Refusal::new(RefusalCode::Other, "the store failed in a way this server does not name"),
        }
    }
}

/// Remove what looks like a credential from text a remote server wrote.
///
/// A server may echo a request header in an error (`Authorization: Bearer
/// abc`); a refusal is read by a model and logged by a client. After a marker
/// (`bearer`, `basic`, `authorization`, `token`, `password`, `secret`, `apikey`,
/// `api_key`, `api-key`) the next word is replaced by `[redacted]`. This is a
/// net, not a proof: the real defence is that tools never put a store's words in
/// a sentence of their own.
pub fn redact(text: &str) -> String {
    // Order matters: "Authorization: Bearer abc" must lose `abc` (the word after
    // `bearer`) before the word after `authorization` (`Bearer`) is replaced.
    const MARKERS: [&str; 9] = [
        "bearer",
        "basic",
        "authorization",
        "access_token",
        "token",
        "password",
        "secret",
        "api_key",
        "api-key",
    ];
    let mut out = text.to_string();
    for marker in MARKERS {
        out = redact_after(&out, marker);
    }
    out
}

fn redact_after(text: &str, marker: &str) -> String {
    // ASCII lowercase keeps every byte offset of the original.
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(marker) {
        let end_of_marker = at + found + marker.len();
        out.push_str(&text[at..end_of_marker]);
        // Separators between the marker and the secret.
        let mut cursor = end_of_marker;
        for (offset, c) in text[end_of_marker..].char_indices() {
            if matches!(c, ' ' | ':' | '=' | '"' | '\'') {
                cursor = end_of_marker + offset + c.len_utf8();
            } else {
                break;
            }
        }
        out.push_str(&text[end_of_marker..cursor]);
        // The secret itself: up to the next separator.
        let mut stop = text.len();
        for (offset, c) in text[cursor..].char_indices() {
            if c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '&' | ';' | ')' | '}') {
                stop = cursor + offset;
                break;
            }
        }
        if stop > cursor {
            out.push_str("[redacted]");
        }
        at = stop;
    }
    out.push_str(&text[at..]);
    out
}
