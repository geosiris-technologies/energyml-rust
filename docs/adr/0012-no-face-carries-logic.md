# 0012 - No face carries logic

Status: accepted

## Context

The library has several faces: a command line, a Python bridge, a WebAssembly
module and applications. Logic written in a face exists only for that face. When
the same decision was written in three places, the copies drifted - colours
resolved by one and not the others, a type filter present in three versions.

## Decision

- The **command line only parses arguments and prints**; the **Python bridge only
  converts** values. Every check, decision and operation lives in the library.
- The order of work is fixed: write it in the library first, then bridge it. The
  bridge carries no feature the Rust library lacks.
- A face that knows something the library does not is a defect in the split.
- The only place a schema version is chosen is the face that starts the program;
  the storage layer receives a registry and never builds one.
- Some wrappers are intentionally thin and tested natively: the Python bridge's
  `session` module is native Rust and tested as such; only the conversion layer is
  Python-specific. The same holds for the WebAssembly module.

## Consequences

- The command line, Python and applications give the same answer to the same
  question, and a check written once protects all of them.
- Replacing the storage (EPC by ETP) changes nothing in the faces.
