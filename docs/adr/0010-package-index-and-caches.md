# 0010 - The package index and caches

Status: accepted

## Context

Several questions cost a full read of a package: "who references this object?",
"what colour does it have?", "which attachments does it have?". Packages can hold
hundreds of thousands of objects. Computing everything up front makes opening
slow; computing nothing makes every question slow.

## Decision

- A package keeps **what it knows of itself**, layer by layer (index of objects
  and links, colour walks, attachment listings), in `energyml-cache`.
- Links and summaries are kept; objects are re-read on demand. The index is
  invalidated by an edit through an overlay of what was changed.
- There is **one implementation**: switching caching off means a slot that is
  thrown away, not another code path.
- The colour rule is the same one for every face (ADR 0009).
- The command-line cache commands are a fixed list.
- Costs are visible: a call that reads everything (like asking who references an
  object) says so in its name, and work is counted in tests, never timed.

## Consequences

- Opening stays cheap; expensive questions are asked deliberately.
- An edit never leaves a stale answer behind, because invalidation is part of the
  write.
