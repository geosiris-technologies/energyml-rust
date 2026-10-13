# 0017 - An operation is a pure function; only `apply` writes

Status: accepted

## Context

Creating an object correctly means assembling identity, references, CRS, array
paths and activity records. Operations had to be offered by a command line, a
page, a workflow engine and a recipe, each wanting to preview what would happen
before anything is written.

## Decision

An **operation** is a pure function from a bag of pieces (`Pieces`) to a `Plan`.
`apply` is the only step that touches storage.

- `plan` takes no store, so it cannot read one; what the caller knows of the
  package arrives resolved.
- An operation **declares** what it needs (slots), takes (pieces), gives (roles)
  and reports; interfaces, the Galaxy generator and the activity template are all
  generated from that one declaration.
- Fitness returns a reason, not a boolean; `offers` lists unfit operations with
  their reason.
- Identities are derived (version-5 UUIDs) from what the input owns, so equal
  inputs name equal objects.
- Every run records an `Activity` against the operation's canonical template.
- A recipe is an ordered list of operations played through the same door into one
  package; each step refers to earlier ones by name.

## Consequences

- Every operation can be previewed ("quote"), tested with values alone and
  replayed to an identical package.
- Adding an operation is one registration; every face gets it.
- All creation rules live in one crate, not in faces.

See [Creating objects](../creating-objects.md).
