# 0004 - Reading policy: `ReadMode`, version resolution, tolerance

Status: accepted

## Context

Real packages break the specification: a required element left out, a value
outside a closed enumeration, a part declared in one release and written in
another. Refusing everything makes such packages unreadable; accepting
everything silently makes wrong data look right.

## Decision

- Absence and nonsense are different news. A `ReadMode` states what to do about
  each. **Strict is the default**; leniency is opt-in and reports one diagnostic
  per invention (with the document's own string when a value was repaired).
- Leniency is about cardinality and unreadable values only. Malformed XML, bad
  encoding and an `xsi:type` on a purely abstract choice stay refused in both
  modes: there is nothing to fall back to.
- The version of a part is resolved from what it declares, over ordered
  candidates, never by guessing from content. A development drop is registered so
  files written against it still open, but is never offered for creation.
- The levels form a ladder: strict, lenient, and finally `repair` for parts no
  parser will touch. What `repair` lists is what the selected mode cannot read.
- Anything intended to be **written back** should be read strictly: a tolerant read
  holds values nobody wrote, and saving puts them on disk.

## Consequences

- Users choose, per command, between reading everything possible and being told
  the truth about what was invented.
- Every departure is visible as a diagnostic.
