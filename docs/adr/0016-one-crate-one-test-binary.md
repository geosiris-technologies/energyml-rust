# 0016 - One crate, one test binary

Status: accepted

## Context

Every file in a crate's `tests/` directory is a separate test binary, and each one
links the generated crates. In a workspace with dozens of such files, a full test
run exhausted the disk of a CI runner with linker failures ("no space left on
device") and made builds slow.

## Decision

- Crates that have many test files keep their tests as modules of a single
  `tests/main.rs` (with `autotests = false`). **A new test file is one `mod`
  line**; without it the file does not run.
- A file that touches state of the whole process (for example a counter of opened
  HDF5 handles) keeps its own binary, declared explicitly.
- Tests are run through the workspace with a filter, never crate by crate: feature
  resolution differs for a single crate and forces a full rebuild.
- Generated code is never reformatted with a workspace-wide formatter; hand-written
  crates are formatted by name.

## Consequences

- Disk use and link time drop considerably.
- Contributors must remember to register a test module.
