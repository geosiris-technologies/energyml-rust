//! The `Tool` trait: what an MCP adapter walks, and what an application adds to.
//!
//! Nothing here names an MCP SDK. An adapter lists [`Registry::tools`](crate::Registry::tools)
//! (name, title, description, input schema, annotations) and forwards a call to
//! [`Registry::call`](crate::Registry::call); that is all it needs.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

use async_trait::async_trait;
use energyml_storage::EnergymlStorage;
use serde_json::{Value, json};

use crate::limits::Limits;
use crate::refusal::Refusal;

/// A type map an embedding application fills.
///
/// A desktop application puts its handle here, an HTTP server the account of
/// the request, so that the tools it registers can reach them. The tools of this
/// crate never read it. One value per type; inserting a second replaces the
/// first.
#[derive(Default)]
pub struct Extensions {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Extensions {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a value, returning the one of the same type it replaced.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|old| old.downcast::<T>().ok())
            .map(|old| *old)
    }

    /// [`Extensions::insert`] for a chain.
    pub fn with<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.insert(value);
        self
    }

    /// The value of this type, if one was inserted.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    /// How many values it holds.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The values are opaque; the count is all that can be said of them.
        f.debug_struct("Extensions")
            .field("len", &self.map.len())
            .finish()
    }
}

/// Everything a call is given besides its arguments.
///
/// `#[non_exhaustive]` so that what an application will need next is not a
/// breaking change of the [`Tool`] trait; built with [`ToolContext::new`].
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct ToolContext<'a> {
    /// The store the call reads.
    pub store: &'a (dyn EnergymlStorage + Send + Sync),
    /// The bounds in force. An argument may lower them, never raise them.
    pub limits: &'a Limits,
    /// What the embedding application attached. Builtin tools never read it.
    pub extensions: &'a Extensions,
}

impl<'a> ToolContext<'a> {
    /// A context for one call.
    pub fn new(
        store: &'a (dyn EnergymlStorage + Send + Sync),
        limits: &'a Limits,
        extensions: &'a Extensions,
    ) -> Self {
        Self {
            store,
            limits,
            extensions,
        }
    }
}

impl fmt::Debug for ToolContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("store", &self.store.store_kind())
            .field("limits", self.limits)
            .field("extensions", self.extensions)
            .finish()
    }
}

/// What a tool needs to know about the store to annotate itself.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoreInfo {
    /// `EnergymlStorage::store_kind` (`EPC`, `etp`).
    pub kind: String,
    /// Whether the store is reached over a network: an "open world" in the MCP
    /// sense, whose content nobody here controls.
    pub remote: bool,
}

impl StoreInfo {
    /// A store of this kind.
    pub fn new(kind: impl Into<String>, remote: bool) -> Self {
        Self {
            kind: kind.into(),
            remote,
        }
    }

    /// Read from the store itself. Only the kind `EPC` (a file) is local; any
    /// other kind, including one this crate has not heard of, is treated as
    /// remote, the cautious reading for an annotation.
    pub fn of(store: &dyn EnergymlStorage) -> Self {
        let kind = store.store_kind();
        Self::new(kind, kind != "EPC")
    }
}

/// The hints MCP lets a tool carry. They help an interface; they protect from
/// nothing (the specification says a client must not rely on them), so the
/// "writes are off" setting is the server's, never an annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Annotations {
    /// A human title, in English.
    pub title: String,
    /// The tool changes nothing.
    pub read_only_hint: bool,
    /// The tool reaches an open world (a remote server).
    pub open_world_hint: bool,
}

impl Annotations {
    /// A read-only tool; open-world when the store is remote.
    pub fn read_only(title: impl Into<String>, store: &StoreInfo) -> Self {
        Self {
            title: title.into(),
            read_only_hint: true,
            open_world_hint: store.remote,
        }
    }

    /// In the shape of MCP's `ToolAnnotations`.
    pub fn to_value(&self) -> Value {
        json!({
            "title": self.title,
            "readOnlyHint": self.read_only_hint,
            "openWorldHint": self.open_world_hint,
        })
    }
}

/// What a tool answers: the structured value, and its serialisation as text.
///
/// The MCP specification asks that a tool returning structured content also
/// return the serialised JSON in a text block, for the clients that read only
/// that. Built with [`ToolOutput::new`], which makes the text, so the two cannot
/// disagree.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ToolOutput {
    /// `structuredContent`.
    pub structured: Value,
    /// The same, serialised: the `text` of a `TextContent` block.
    pub text: String,
}

impl ToolOutput {
    /// An answer from its structured value.
    pub fn new(structured: Value) -> Self {
        // A `Value` always serialises; the fallback is only for the compiler.
        let text = serde_json::to_string(&structured).unwrap_or_else(|_| "null".to_string());
        Self { structured, text }
    }

    /// A refusal as an answer, for an adapter that sets `isError: true`.
    pub fn from_refusal(refusal: &Refusal) -> Self {
        Self::new(refusal.to_value())
    }
}

/// One tool of the server.
///
/// An implementation converts the arguments, calls the storage traits or a
/// library function, and shapes the result. It holds no logic of its own: what
/// is missing is a function of the library, then the tool calls it.
///
/// Native only, like the crate: the futures are `Send`.
// `async_trait` marks the boxed future `#[must_use]`, which clippy reads as
// ours (`double_must_use`): the same allowance as `EnergymlStorage`.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name the model calls: `energyml_` followed by a snake_case verb.
    /// Stable: a name never disappears or changes meaning.
    fn name(&self) -> &'static str;

    /// A short human title, in English.
    fn title(&self) -> &'static str;

    /// What the model reads. In English, and it must say that the contents of
    /// the fields of an answer are data of the store, not instructions.
    fn description(&self) -> &'static str;

    /// The JSON Schema (2020-12) of the arguments. Objects are
    /// `additionalProperties: false`.
    fn input_schema(&self) -> Value;

    /// The JSON Schema of the answer, when the tool promises one. The builtin
    /// tools promise none in this version.
    fn output_schema(&self) -> Option<Value>;

    /// The hints for this store. Provided: read-only, open-world when the store
    /// is remote, titled by [`Tool::title`].
    fn annotations(&self, store: &StoreInfo) -> Annotations {
        Annotations::read_only(self.title(), store)
    }

    /// Run the tool. A failure the model can act on is a [`Refusal`]; only an
    /// unknown tool or a malformed request is a protocol error, and that is the
    /// registry's to say.
    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, Refusal>;
}
