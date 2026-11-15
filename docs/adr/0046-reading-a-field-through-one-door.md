# 0046 - Reading a field of a reflected object goes through one door

Status: accepted

## Context

Every reader of the model that walks a `&dyn Reflect` asks the same few
questions: what text does this field hold, which number, is the flag set, which
nested object, which objects of a repeated field. About forty private functions
answered them, one set per module (`text`, `scalar_f64`, `flag`, `objects`,
`field_object`, `string_of`, `number_of`...), in `energyml-geometry`,
`energyml-domain`, `energyml-storage` and `energyml-build`.

The copies did not agree, and the same field therefore read differently
depending on which check looked at it:

- a blank string: kept by one copy, dropped by another when empty, by a third
  when only whitespace;
- a fractional double asked for as an integer: truncated by some, refused by
  others;
- a string asked for as a number: parsed by one copy, refused by the rest;
- a field of the wrong shape: sometimes `None`, sometimes a default.

## Decision

- **One module, `energyml_core::reflect::fields`.** It has no native dependency
  (it builds for `wasm32`). The private copies are deleted; where a module kept
  a local name for readability it re-imports the door's function under that name,
  never a second implementation.
- **The policy is stated once, in the module's documentation:**
  - absent (no such field, unset, undeclared name) is `None`, or an empty `Vec`
    for the plural readers; never zero, `false` or the empty string;
  - a blank string (empty or whitespace only) is absent for the single `text`
    reader; the plural readers keep every element, because in a list the position
    is the information;
  - a field of the wrong shape is `None`: the strict readers never parse a string
    as a number, and a double is an integer only when it is finite, whole and in
    range;
  - reading a number out of text is a **named variant** (`number_lexical`,
    `integer_lexical`, `flag_lexical`), asked for only where a schema version keeps
    the value as text;
  - a caller that must refuse says so: the `require_*` readers return a named
    error, `MissingRequired` for an absent field and `TypeMismatch` for a field of
    another shape, instead of the caller inventing a default.
- **The reflection walks of `identity.rs` share one traversal.** `collect_refs`,
  `nested_objects` and `collect_refs_by_owner` are visitors of a single `walk`
  (references, kept-verbatim subtrees, entered objects). The dotted path is one
  buffer, pushed and truncated, not a `format!` per field.
- **An error that names a type borrows it.** The geometry readers passed
  `Box::leak`ed copies of `&'static str` type names to their errors; the names
  are already `'static` and are passed as they are.

## Consequences

- A change of policy (what a blank means, what an integer is) is made in one
  place and applies to every reader.
- A reader that wants the old lenient behaviour on a field kept as text asks for
  the lexical variant by name; it does not get it by accident.
- Behaviour that changed where the copies disagreed:
  - `text` now drops a blank string in every reader (it used to be kept by
    `integrity`, `property`, `values` and the geometry `well`/`parent` readers),
    and returns the wire text of a non-string scalar in every reader (the
    geometry and `values` readers were string-only);
  - an integer read from a fractional double is now `None` (it was truncated by
    the geometry, `values` and `crs` readers); a whole double still reads;
  - `property` no longer truncates a non-negative fractional double to a count.
- A new reader of the model does not write a helper: it imports the door.
