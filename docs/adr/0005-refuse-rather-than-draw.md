# 0005 - Refuse rather than draw; `FeatureNotEnabled`

Status: accepted

## Context

An empty result looks exactly like a program that works and has nothing to show;
a wrong result looks exactly like a right one. A point placed in feet read as
metres, a surface drawn from loosely connected lines, a missing array backend
reported as an empty array: each produces a plausible output that nobody
questions.

## Decision

- Where the specification is ambiguous or the data insufficient, the answer is a
  **named refusal**, not a guess. When a guess is finally settled, it is settled
  by evidence written next to the code.
- Absence and error are distinct: a missing capability (an absent array backend, a
  build without HDF5) returns `FeatureNotEnabled`, never an empty array. A reader
  that cannot read something returns an error naming the type, never nothing.
- A batch carries on past one refusal and reports each, with the object and the
  reason.
- Choices the library makes where the specification is silent are recorded in the
  house rules registry, with their reasons.

## Consequences

- Users sometimes get an error where a lenient tool would give a picture. The
  error names the cause, and a rule or an option exists to go further.
- No refusal is silent: counts of set-aside samples, unlabelled marks or skipped
  objects are part of the answer.
