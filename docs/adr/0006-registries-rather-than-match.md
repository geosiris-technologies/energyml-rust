# 0006 - Registries rather than `match`

Status: accepted

## Context

Several parts of the library have a list that will grow: array backends,
representation readers, exporters, operations, proportion modes of a well sheet,
types registered per schema version. A `match` over a closed list forces every new
entry to change the library.

## Decision

Where the list will grow, define a **trait and a registry**. Built-ins are pushed
by default; entries added by the caller are tried **first**, so that a built-in is
replaceable and not only extensible.

## Consequences

- A new export format, a new array backend, a new operation or a new schema
  release is an addition, not an edit of existing code.
- Interfaces build themselves from the registry (the list of export formats, of
  operations, of provider filters), so they show a new entry without change.
- Ordering matters and is part of the contract: the first match wins.
