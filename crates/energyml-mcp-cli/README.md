# energyml-mcp-cli

The `energyml-mcp` program: a **read-only** [MCP](https://modelcontextprotocol.io) (Model Context Protocol) server over one EnergyML EPC package, on stdio. Register it in Claude Code, Claude Desktop or any MCP client and a language model can orient itself in a package, list and find objects, read one as JSON, follow its references and read a bounded window of an array.

The program is a face: it parses arguments, opens the package the way `energyml` does, and calls `energyml_mcp::server::serve_stdio`. The tools, the limits and the refusals are in [`energyml-mcp`](../energyml-mcp/README.md).

```sh
cargo install energyml-mcp-cli        # the package installs the `energyml-mcp` binary
energyml-mcp model.epc                # waits for an MCP client on stdin: run it from the client
energyml mcp model.epc                # the same, through the `energyml` command
energyml-mcp model.epc --h5 data.h5   # offer an HDF5 file for the arrays
```

Register it in Claude Code:

```sh
claude mcp add energyml -- energyml-mcp /path/to/model.epc
```

The [MCP page of the documentation](../../docs/mcp.md) has the Claude Desktop configuration, the table of tools, the limits and the refusals.

## Options

| Option | Environment variable | Default | Accepted |
|---|---|---|---|
| `<file.epc>` | | required | an EPC package |
| `--h5 [NAME=]PATH` | | none | repeatable; same switch as `energyml` |
| `--max-response-bytes` | `ENERGYML_MCP_MAX_RESPONSE_BYTES` | 65536 | 1024 to 16777216 |
| `--max-list` | `ENERGYML_MCP_MAX_LIST` | 500 | 1 to 10000 |
| `--max-header-scan` | `ENERGYML_MCP_MAX_HEADER_SCAN` | 2000 | 1 to 100000 |
| `--max-window-elements` | `ENERGYML_MCP_MAX_WINDOW_ELEMENTS` | 10000 | 1 to 1000000 |
| `--max-object-bytes` | `ENERGYML_MCP_MAX_OBJECT_BYTES` | 262144 | 1024 to 16777216 |
| `--timeout-seconds` | `ENERGYML_MCP_TIMEOUT_SECONDS` | 30 | 1 to 3600 |

A call argument can lower a limit, never raise it. An option outside its accepted range is refused at start-up (exit status 2).

## Features

| Feature | Default | Effect |
|---|---|---|
| `hdf5` | no | read the arrays; needs the HDF5 C library at build time |
| `hdf5-static` | no | the same, with HDF5 built from its sources and linked in |

Without `hdf5`, `energyml_read_array_window` refuses `feature_not_enabled`: the data is not absent, the means to read it is.

## stdout is the protocol

The program writes **only** JSON-RPC messages on stdout. The start-up line and every error go to stderr, and a failure to start (a missing file, a bad option) leaves stdout empty and the exit status non-zero.

## Documentation

- [MCP server page](../../docs/mcp.md) and [ADR 0021](../../docs/adr/0021-own-tool-trait-rmcp-as-adapter.md)
- The ETP counterpart, `retp mcp`, serves the same tools for an ETP server (see the [`retp`](https://github.com/geosiris-technologies/retp) repository).

## Licence

Apache-2.0. See [`LICENSE`](../../LICENSE).
