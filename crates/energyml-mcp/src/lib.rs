//! The tools of a generic, read-only MCP server over the EnergyML storage traits.
//!
//! This crate is a **face**, like the command line and the bridges: it converts
//! arguments, calls [`energyml_storage::EnergymlStorage`] or a function of the
//! library, and shapes the result. It holds no logic about EnergyML, and it
//! knows no MCP SDK: [`Tool`] and [`Registry`] are ours, so that an adapter to
//! `rmcp` (or to a hand-written JSON-RPC loop) only has to walk the registry:
//!
//! 1. `tools/list`: [`Registry::tools`], each with [`Tool::name`],
//!    [`Tool::title`], [`Tool::description`], [`Tool::input_schema`],
//!    [`Tool::output_schema`] and [`Tool::annotations`];
//! 2. `tools/call`: [`Registry::call`], whose [`CallError::UnknownTool`] is a
//!    protocol error and whose [`CallError::Refused`] is a tool execution error
//!    (`isError: true`) carrying [`Refusal::to_value`];
//! 3. the store of a request: a [`StoreProvider`] ([`FixedStore`] for stdio).
//!
//! ```
//! use energyml_mcp::{Extensions, Limits, Registry};
//!
//! let registry = Registry::builtin();
//! let names: Vec<_> = registry.tools().map(|tool| tool.name()).collect();
//! assert_eq!(names[0], "energyml_store_info");
//! // A call needs a store: `registry.call(name, &ToolContext::new(&*store, &Limits::default(), &Extensions::new()), args)`.
//! let _ = (Limits::default(), Extensions::new());
//! ```
//!
//! # What an answer promises
//!
//! - **Bounded, and it says so.** Every answer carries `truncated` and, when
//!   true, `truncation { reason, limit, how_to_get_more }`; every list carries
//!   `total`, `returned` and `next_cursor`. The bounds are [`Limits`]; an
//!   argument can lower one, never raise it.
//! - **Refused, never drawn.** What a store cannot do is a [`Refusal`] with a
//!   code, never an empty list ([`RefusalCode`]).
//! - **One door per access.** Every `uri` argument goes through
//!   `EtpUri::parse`; a bare `uuid` is accepted at the entry only and turned at
//!   once into a full URI by `energyml_storage::search::resolve_uuid`.
//! - **Store text is data.** Titles, server messages and custom data stay in
//!   data fields of the answer and are never joined into a sentence of the
//!   server; each tool description says so to the model.
//!
//! # Extending
//!
//! An application registers its own tools with [`Registry::with_tool`]; they are
//! listed and tried before the builtin ones, and may shadow one. What they need
//! beyond the store (an application handle, an account) travels in
//! [`Extensions`], available to a tool as [`ToolContext::extensions`].
//!
//! # Native only
//!
//! The tools are `async_trait` with `Send` futures, like `EnergymlStorage` off
//! wasm. There is no runtime here: the timeout of [`Limits::call_timeout`] is
//! enforced by the adapter that owns one.

#![forbid(unsafe_code)]
// Every function of a tool answers `Result<_, Refusal>`, and a refusal carries
// the sentences the model reads (message, hint, the store's own words, the
// candidates): boxing it would only move those strings. The same allowance as
// `energyml-section`'s readers.
#![allow(clippy::result_large_err)]
#![warn(missing_docs)]

#[cfg(target_arch = "wasm32")]
compile_error!("energyml-mcp is native only: its tools are Send futures and a server needs a runtime");

mod limits;
mod provider;
mod refusal;
mod registry;
mod tool;
pub mod tools;

pub use limits::Limits;
pub use provider::{FixedStore, RequestInfo, StoreProvider};
pub use refusal::{Refusal, RefusalCode, redact};
pub use registry::{CallError, Registry};
pub use tool::{Annotations, Extensions, StoreInfo, Tool, ToolContext, ToolOutput};
