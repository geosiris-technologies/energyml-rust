# 0019 - Synthetic data is flagged and sourced

Status: accepted

## Context

Some operations of `energyml-build` write objects nobody measured: a horizon laid
at a stated thickness from another one, a fault drawn from sticks, a property
filled by a law of depth, a throw, a velocity law between depth and time. Once
written, such an object looks exactly like a measured one, and a number with no
origin looks exactly like a fact. A reader downstream (a viewer, a check, another
tool) has no way to tell, and a plausible constant that nobody can trace becomes
a value other people rely on.

## Decision

Two rules, applied by every operation that invents something.

1. **An invented object says so.** It carries an `ExtensionNameValue` named
   `synthetic` with the value `true` (the field exists on every eml 2.3
   `AbstractObject`), whose `Description` says what is invented and how, for
   example "laid by `horizon-from-law` at a stated thickness from another
   horizon, not measured". The object's `Citation/Description` also carries a
   sentence stating the law and its source; it stays within the 2000 characters
   that field allows, and a source that would not fit is refused.
2. **No constant without a source.** A value that is not measured comes with a
   `source` setting that is **required**, and a blank one is refused by name. The
   source is either a document with its page or paragraph, or the statement that
   the value is plausible and why. A value presented as real cites its document;
   a plausible one is declared synthetic. The source text is written in the
   object, but it takes no part in any derived identity: rewording or translating
   it never renames an object.

The plan reports the mark in its notes (a note with the key `synthetic`), so the
person sees it before anything is written. Operations that invent only when told
to (a `synthetic` parameter) write the mark only then. The operations that are
meant to produce synthetic models are grouped under the `synthetic` category in
listings.

## Consequences

- A synthetic object can be found by any reader that lists extension values, and
  filtered out of anything that must be measured.
- Every invented number can be traced to a sentence a person wrote.
- An operation that invents must ask for a source; one more required setting is
  the price, paid once per recipe step.
- The mark describes the object, not the package: a package mixing measured and
  synthetic objects stays readable object by object.

See [Creating objects](../creating-objects.md) and
[ADR 0018](0018-synthetic-reference-model.md) for the reference model that uses
these operations.
