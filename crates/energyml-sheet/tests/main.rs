//! The well sheet's tests, in one binary (ADR-0016: one crate, one test
//! binary - a new test file is a `mod` line here, or it does not run).
//!
//! Run them alone: `cargo test --workspace --test sheet <filter>`.

mod draw;
mod layout;
mod read;
mod svg;
