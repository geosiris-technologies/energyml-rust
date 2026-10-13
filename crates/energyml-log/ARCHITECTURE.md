# energyml-log: architecture

## Purpose and position

The one place a `tracing` subscriber is installed. It is a leaf: pure Rust, no native dependency, wasm included. Libraries in this workspace open spans and send events; the command line, the desktop application or an embedding program calls `init` (or `init_reloadable`) once.

## Rules

**The libraries emit, they never configure** — no library crate installs a subscriber. Whether anything is shown, how, and at which level is the face's decision, made in one place.

**Off by default** — with no filter argument and no `ENERGYML_LOG` in the environment, `init` installs nothing: no subscriber, no cost, and the output of every command is what it was. An explicit argument wins over the variable.

**Always stderr, never stdout** — stdout carries results that scripts read; logging must not be able to corrupt them.

**The filter is `tracing_subscriber::EnvFilter` syntax** (`info`, `energyml_build=debug,energyml_storage=info`, `off`); `ENERGYML_LOG_FORMAT` chooses `text` or `json`.

**A span says how long it took** — closing a span writes `time.busy` and `time.idle`, so timing needs no instrumentation beyond opening the span.

**The level can change while the program runs** — `LogHandle::set_filter` swaps the filter live, from off to `debug` and back, even when the program started with logs off, provided it installed through `init_reloadable`. This is what a "log level" menu calls.

**`log` records are bridged** — crates that still use the `log` crate go through the same filter once a subscriber is installed.

## Deliberately not here

No log file management, no rotation, no network sink, no instrumentation of its own.

## How it is tested

`tests/reload.rs` checks that the filter changes while the program runs, including from a start with logging off.
