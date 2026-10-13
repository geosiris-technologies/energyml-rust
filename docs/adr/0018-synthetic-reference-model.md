# 0018 - A synthetic reference model rather than an external corpus

Status: accepted

## Context

A public test suite that depends on files nobody versions or reviews, or whose
licence is unknown, cannot be relied on and cannot be reproduced by a contributor.
Real files remain the best test of interoperability, but not all of them can be
redistributed.

## Decision

- The public suite is built from: (A) a **synthetic, documented reference model**
  (Lutetia) kept in its own repository and read at a pinned commit, (B) small
  reference packages written by other tools, fetched from their original
  repositories **in CI only** and pinned by commit and SHA-256, (C) data fabricated
  inside tests, and optionally (D) packages written by other libraries during CI.
- No test goes on the network; a missing data set fails CI instead of skipping.
- A model written by this project's own operations checks functions, not
  interoperability; interoperability is the job of (B).
- A **private campaign on real packages** runs separately, and the public
  documentation says so.

## Consequences

- Contributors can run the whole public suite from a clean clone.
- The reference model is itself a public asset: documented, citable and usable by
  other implementations as a test set.
- Some behaviours are only exercised privately; the testing page lists the gap.

See [Testing](../testing.md).
