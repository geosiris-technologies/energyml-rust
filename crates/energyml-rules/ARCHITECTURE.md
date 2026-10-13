# energyml-rules: architecture

## Purpose and position

The registry of the **house rules**: what this library decides where the specification is silent, ambiguous or only interpreted. It is a leaf with **no runtime dependency** (wasm included). Rules are TOML files compiled by `build.rs` into statics, and `id::<NAME>` is one constant per rule, so a misspelt id does not compile and nothing is parsed at run time. Any crate cites a rule by id; a face can list them all.

Related decisions: [ADR-0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR-0011](../../docs/adr/0011-tests-follow-the-specification.md).

## What a rule holds

`rules/<theme>.toml` (one file per theme, English, the source; `themes.toml` declares the themes). A rule has an id, a status (`active`, `designed`, `superseded`), what the specification does (`silent`, `ambiguous`, `interpreted`, `extends`, `restates`), a level, a text, a reason, who decided and when, quoted references and anchors (where it shows). French translations (`rules/fr/`, behind the default `fr` feature) are optional per rule and carry id, title, text and reason only; an untranslated rule answers in English and says so.

## Rules

**A new rule enters the registry in the same commit as the decision or the code that makes it** — a rule touched is a rule rewritten here. Other crates' documentation refers to the registry and does not copy a rule.

**A rule that changes meaning is a new id** — with `supersedes`, the old one passing to `superseded`; history is information. Ids have the shape `house/<theme>/<name>`; two ids published before that shape remain flat and no other flat id is accepted.

**`designed` is a rule decided and not yet coded** — it says so wherever it is shown. Moving it to `active` belongs to the commit that codes it.

**References are quoted strings, never read at run time** — the loader checks their form (a specification document from a closed list, a section / page / line, a repository-relative path), not the document. A reference is a citation a reader can verify, not a dependency of the build.

**The loader is shared between the build and the tests** — `build/loader.rs` is `include!`d by `build.rs` and by the tests, so what the build refuses and what the tests try to break are the same code. Every guard answers with a named error (file, rule, field). Guards are tested with fabricated files, because the shipped registry passes them all by construction; each test says what to break to turn it red.

**No logic belonging to a face** — filtering and JSON selection live here, so the command line, wasm and Python only convert.

## Deliberately not here

No runtime dependency, no rule enforcement (the code that makes a decision cites its rule; this crate does not run it), and no rendering of the rules.

## How it is tested

`tests/loading.rs` breaks the loader with fabricated files; `tests/registry.rs` checks the shipped registry; `tests/crosscheck.rs` crosses the `house/` ids with the domain crate's integrity checks in both directions (a dev-dependency, so no cycle the day the domain depends on this crate). The crate must keep compiling for `wasm32`.
