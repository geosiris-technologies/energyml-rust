# 0011 - Tests follow the specification, not a corpus

Status: accepted

## Context

A real package can contain non-conforming data. Pinning a value measured from one
turns its defect into a rule the suite then defends. Separately, a component that
checks itself agrees with itself, and a test that cannot fail proves nothing:
checks that passed on empty sets have been found in this project.

## Decision

- **The shape of an assertion comes from the specification; the values come from
  the data.** Only the first has authority. A test that states a rule cites the
  document, page and paragraph in a comment.
- **Reference documents are cited, never read at run time.** Specifications and
  schemas are not opened by tests; a test restates the rule instead.
- **Triangulate.** Never ask a component to verify itself. Each cross-check
  confronts two independent sources of the same fact, at least one of which is not
  code written here: the bytes of the HDF5 file, the part names of the zip, the
  counts the XML declares.
- **A test must be able to fail.** When adding one, ask what would have to be
  broken for it to go red; if the answer is "nothing", break something on purpose
  to find out.
- An unexplained discrepancy is pursued, not pinned.
- Real files remain the only real test, so they are exercised in a separate
  private campaign; the public suite does not depend on them (ADR 0018).

## Consequences

- Tests stay valid when a producer is fixed.
- Writing a cross-check costs more than writing an assertion; that is the point.
