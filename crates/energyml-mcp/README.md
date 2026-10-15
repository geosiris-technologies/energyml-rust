# energyml-mcp

The tools of a generic, **read-only** [MCP](https://modelcontextprotocol.io) (Model Context Protocol) server over the EnergyML storage traits. Point it at any `EnergymlStorage` (an EPC file, an ETP server) and a language model can orient itself, list and find objects, read one as JSON, follow its references, and read a bounded window of an array.

This crate is only a face over `energyml-storage`: each tool converts its arguments, calls a trait method or a library function, and shapes the result. It holds no logic about EnergyML, and it depends on **no MCP SDK and no async runtime**. It defines its own `Tool` trait and `Registry`; an adapter to an SDK (`rmcp`) or to a hand-written JSON-RPC loop only has to walk the registry, so replacing the SDK touches no tool and no test.

The stdio server is the feature `server` (an adapter over the `rmcp` SDK, ADR-0021); the binary `energyml-mcp` (package `energyml-mcp-cli`, so `energyml mcp file.epc`) and `retp mcp wss://...` (the `retp` repository) serve these tools. See the book's page `docs/mcp.md`. Native only: the tools are `Send` futures, like the storage traits off wasm.

## What an adapter does

```rust
use energyml_mcp::{Extensions, Limits, Registry, ToolContext};

let registry = Registry::builtin();

// tools/list: each tool in registration order, each name once.
for tool in registry.tools() {
    let _ = (tool.name(), tool.title(), tool.description(), tool.input_schema(), tool.output_schema());
    // tool.annotations(&StoreInfo::of(store)) for readOnlyHint / openWorldHint
}

// tools/call: the store comes from a `StoreProvider` (`FixedStore` for stdio).
let limits = Limits::default();
let extensions = Extensions::new();
let ctx = ToolContext::new(&*store, &limits, &extensions);
match registry.call("energyml_list_resources", &ctx, args).await {
    Ok(output) => { /* output.structured, and output.text (the same, serialised) */ }
    Err(energyml_mcp::CallError::UnknownTool(_)) => { /* a protocol error */ }
    Err(energyml_mcp::CallError::Refused(refusal)) => { /* isError: true, refusal.to_value() */ }
}
```

The timeout is **not** applied here (there is no runtime in this crate): the adapter that owns the executor races each `Registry::call` against `Limits::call_timeout` and answers with a `timeout` refusal.

## The tools

All are read-only. Names carry the `energyml_` prefix. Every list carries `total`, `returned`, `truncated` and `next_cursor`; every answer carries `truncated` and, when true, `truncation { reason, limit, how_to_get_more }`.

| Tool | What it does | Calls |
|---|---|---|
| `energyml_store_info` | Kind, capabilities, dataspace and type counts, whether arrays are readable, diagnostics, the limits in force. Call it first | `store_kind`, `capabilities`, `dataspaces`, `supported_types`, `diagnostics` |
| `energyml_list_dataspaces` | Dataspaces with their URIs. `custom_data`: keys only unless the operator shows values | `dataspaces` |
| `energyml_list_resources` | Objects of a dataspace, or reachable from an object. `types`: exact or `family.*`, case sensitive | `resources_in` |
| `energyml_find_resources` | Case-insensitive substring search: `title_contains`, `type_contains` (distinct from `types` above), `uuid_prefix` | `search::find`, `headers` |
| `energyml_describe_object` | One object as EnergyML JSON (`$type` on every object), bounded; XML when the store lists it, refused by name otherwise | `get_object`, `header`, `references`, `get_raw`, `truncate` |
| `energyml_get_references` | Targets, sources or both up to a depth; references to absent objects as `dangling` | `resources_in`, `references`, `resources` |
| `energyml_list_arrays` | Arrays of an object with shape and type, no values; an unreadable array is listed with the reason | `get_object`, `arrays_of`, `array_metadata` |
| `energyml_read_array_window` | A bounded window with statistics and a preview; never the whole array | `array_metadata`, `window::read_window` |

A `uri` argument always goes through `EtpUri::parse`. A bare `uuid` is accepted at the entry of `describe_object` and `list_arrays` only, and turned at once into a full URI by `search::resolve_uuid`; everything returned carries full URIs.

## Limits

Settings of whoever runs the server, never of a tool. An argument may **lower** a bound and never raise it.

| Setting | Default | Role |
|---|---|---|
| `max_response_bytes` | 65 536 | hard bound of any answer; larger is refused `too_large`, lists stop earlier and say so |
| `max_list` | 500 | ceiling of every `limit` |
| `max_header_scan` | 2 000 | headers a title search reads without `scope` or `type_contains` |
| `max_window_elements` | 10 000 | elements one `energyml_read_array_window` reads |
| `max_object_bytes` | 262 144 | ceiling of `max_bytes` of `energyml_describe_object` |
| `call_timeout` | 30 s | enforced by the adapter |
| `reveal_custom_data` | off | show the values of a dataspace's `custom_data` (access lists, legal tags) |

## Refusals

A tool that cannot answer returns a refusal, never an empty list: `{"refusal": {"code", "message", "store"?, "operation"?, "hint"?, "detail"?, "candidates"?}}` with `code` one of `not_found`, `ambiguous`, `bad_uri`, `unsupported`, `feature_not_enabled`, `external_data`, `too_large`, `remote`, `bad_arguments`, `timeout`, `other`. `From<energyml_storage::error::Error>` is the mapping table. A missing capability (`feature_not_enabled`, HDF5 not compiled in) is not the same fact as absent data.

## Untrusted data

Titles, server messages and custom data come from the store and may contain instructions aimed at the model. They are returned only inside data fields (`title`, `detail`, `candidates`...), as the whole value of the field, never inside a sentence written by the server, and every tool description tells the model so. Credentials that a remote server echoes in an error are removed from `detail`.

## Adding tools

An application adds its own tools next to these:

```rust
let registry = Registry::builtin().with_tool(Box::new(MyTool));
```

Caller tools are listed first and tried first; a caller tool with the name of a builtin one **shadows** it (the name is listed once, and the caller's tool answers). What a tool needs beyond the store travels in `Extensions`, a type map the application fills (an application handle, the account of an HTTP request) and the tool reads from `ToolContext::extensions`. The builtin tools never read it.

```rust
let mut extensions = Extensions::new();
extensions.insert(MyAppHandle::new());
// ... in MyTool::call: ctx.extensions.get::<MyAppHandle>()
```

## Documentation

- API reference: <https://docs.rs/energyml-mcp>
- Repository documentation: [`docs/`](../../docs/) and [`ARCHITECTURE.md`](../../ARCHITECTURE.md)

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
