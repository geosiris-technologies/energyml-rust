# 0021 - Our own `Tool` trait; the MCP SDK is an adapter behind a feature

Status: accepted

## Context

The MCP server offers read-only tools over the storage traits. Rust has an
official SDK for it, `rmcp`, which defines tools with macros on a handler
(`#[tool_router]`, typed parameters) and brings tokio.

Three facts weigh against building the server on those macros:

- **The SDK moves.** `rmcp` went through three major versions in five months
  (1.0 in March 2026, 2.0 in June, 3.0 in July) and keeps tightening behaviour
  between minors. The MCP specification itself changed shape (a session with an
  `initialize` handshake, then a stateless revision), and clients still speak
  both.
- **The tool list will grow, and not only by us** (ADR-0006). An application
  embedding the library adds the tools that drive its own views; a server adds the
  tools its users' rights allow; a later version adds writing, validation,
  export. A set of tools fixed at compile time by macros on one `impl` cannot be
  extended by a caller, and cannot put the caller's tool before a builtin one.
- **tokio must not reach the libraries.** Neither workspace has tokio today, the
  ETP client is blocking, and `energyml-core` is wasm-clean (ADR-0007). A crate
  that only wants the registry (a desktop application, a test, a different
  transport) must not pay for an async runtime.

## Decision

- **`Tool` and `Registry` are ours, and SDK-independent.** A tool is a value with a
  name, a title, a description, a JSON Schema for its arguments, hints, and an
  `async call(&ToolContext, args)` that answers a `ToolOutput` or a `Refusal`.
  The registry lists tools in registration order and tries what the caller
  registered first (`Registry::with_tool`). What a tool needs beyond the store
  travels in `ToolContext::extensions`. `StoreProvider` is the one point where the
  store of a request comes from (a fixed store for stdio, a per-request store
  behind HTTP).
- **`rmcp` is an adapter** in `energyml-mcp`'s `server` module, behind a Cargo
  feature `server` that is off by default. It implements `ServerHandler` by hand
  (`list_tools` walks the registry, `call_tool` calls it) and nothing else. **No
  `rmcp` type appears in a public signature** of `energyml-mcp`; errors carry the
  SDK's messages as text.
- **tokio is confined to the feature.** `serve_stdio` builds its own
  single-threaded runtime and blocks the calling thread, so callers see a plain
  function. Each tool call runs on a blocking thread around
  `futures_executor::block_on`, because the ETP store blocks inside its `async`
  methods, and is raced against `Limits::call_timeout`; a timeout answers the
  refusal `timeout`.
- **Two kinds of failure, as the specification has them.** An unknown tool is a
  JSON-RPC protocol error. Everything else a tool declines is a tool result with
  `isError: true`, carrying `{"refusal": ...}` (ADR-0005): the model can read it
  and correct itself. Never an empty list.
- **The tool schemas are written by hand** as `json!` values, not derived from
  Rust types, so that a snapshot test of `tools/list` guards the contract
  clients see and a refactor cannot move it silently.

## Consequences

- Replacing the SDK, or writing the JSON-RPC loop by hand for stdio, rewrites one
  file and touches no tool and no test of a tool. The adapter is a few hundred
  lines; a new `rmcp` major costs an afternoon in that file.
- The adapter is tested with the SDK's own client over an in-memory duplex
  stream, so the server and the client are not both written by us.
- The server can be handed a registry with the caller's tools first, at no cost to
  the builtin ones; a hosted server (`retp mcp`, a future HTTP server) shares the
  same tools by providing a different `StoreProvider` and transport.
- We write the tool schemas ourselves; a schema derivation crate is not a
  dependency.
- A call that times out is **abandoned, not stopped**: a blocking thread cannot be
  cancelled. It ends on its own and its answer is dropped. At exit the process
  gives a stuck call a short grace period and leaves anyway.
- Features of the SDK we do not use (resources, prompts, subscriptions, tasks) are
  not offered; the server advertises the `tools` capability only. Offering one
  later is an addition to the adapter and, if it needs a trait, to this crate.
- The contract with clients is the tool names and their arguments, not the Rust
  API: a tool name never disappears, an argument added later is optional, an
  incompatible change is a new tool name.
