//! Every integration test of `energyml-mcp-cli`, as **one** binary.
//!
//! A new test file is a `mod` line here, otherwise it does not run (the rule of
//! the other crates: one binary per crate, `autotests = false`).
//!
//! The tests start the real program (`CARGO_BIN_EXE_energyml-mcp`) on a package
//! they fabricate in a temporary directory from the storage contract's own model,
//! and talk to it in JSON-RPC lines. They read no file of the repository and no
//! corpus. The protocol itself is proved in-process in `energyml-mcp`
//! (`tests/server.rs`, with the SDK's own client); what is proved here is the
//! program: its arguments, its stdout (protocol only) and its exit codes.

mod arguments;
mod session;
mod support;
