# energyml-build-derive - architecture

Related decision: [ADR 0017](../../docs/adr/0017-operation-is-a-pure-function.md) (an operation is a pure function; its declaration is one fact).

## Purpose and position

The procedural macros behind `energyml-build`'s typed operations. An operation's parameters, choices and inputs are **declared once**, as Rust types; the form a user fills in, the reading of the answers and the activity template all come from that single declaration. The crate is a `proc-macro` crate and depends only on `syn`, `quote`, `proc-macro2` and `heck`.

Three derives:

- `Choice` - a unit enum becomes a closed list of words (`thin-plate`);
- `Params` - on a struct, one slot per field (or a nested block with `flatten`); on an enum with `#[params(tag = "…")]`, a **union**: one choice slot, then each case's slots, each acting only under its case;
- `Inputs` - one input slot per field, bound by the framework.

## Rules

- **The field is the declaration.** Its type is the kind of answer, its doc comment's first paragraph is the label, and `default = …` is written once. Before this, a parameter was declared in `needs()` and read back by name in `plan()`, with its default written a second time beside its suggestion: two places for one fact.
- **`plan` reads no name.** It receives the structures already read and bound; it does not know the naming suffixes of companion objects.
- **The generated code names `::energyml_build`**, and `energyml-build` says `extern crate self as energyml_build` so the derives also compile inside it.
- **The version of this crate moves with `energyml-build`.** The generated code relies on that crate's internal types; do not depend on this crate alone.

## Not done

It does not implement `Operation` or register anything: adding an operation is a file implementing the trait plus one line in the registry, in `energyml-build`.

## Tests

Exercised through `energyml-build`'s tests: each operation's declaration is projected to its activity template and compared against the committed template, so a derive that drops or reorders a slot makes a test fail there.
