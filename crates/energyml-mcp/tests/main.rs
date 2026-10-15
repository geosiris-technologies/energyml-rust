//! Every integration test of `energyml-mcp`, as **one** binary.
//!
//! A new test file is a `mod` line here, otherwise it does not run (the rule of
//! the other crates: one binary per crate, `autotests = false`).
//!
//! The tests read no file: the package is a zip built in memory from the
//! storage contract's own model (`energyml_storage::contract`), and the arrays
//! come from a fake store that wraps it. Nothing here needs a runtime beyond
//! `futures_executor::block_on`.

// The helpers answer `Result<_, Refusal>` like the tools: the crate's allowance.
#![allow(clippy::result_large_err)]

mod arrays;
mod extensions;
mod limits;
mod refusals;
mod registry;
mod snapshot;
mod support;
mod tools;
mod truncation;
mod untrusted;
