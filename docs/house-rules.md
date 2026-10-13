# House rules

Energistics standards leave questions open: a producer may omit a field, two
readings of a sentence are possible, a unit string means different things in two
releases. Where the specification is **silent, ambiguous or only interpreted**,
this library makes a decision. A **house rule** is such a decision, when it
changes what a user sees, reads or is refused and no specification dictates it
word for word.

Those decisions are not left scattered in code comments. They are in a registry,
the `energyml-rules` crate, so that:

- a user can ask the tool what it decided, and why;
- code cites a rule by an identifier, and a misspelt identifier **does not
  compile**;
- an interface can show "this is a house rule" next to the answer it affects.

## What a rule says

Each rule is a record in a TOML file, one file per theme:

| Field | Meaning |
|---|---|
| `id` | `house/<theme>/<name>` |
| `status` | `active` (decided and coded), `designed` (decided, not coded yet - says so on the button that shows it), `superseded` (kept for history) |
| stance | what the specification does: `silent`, `ambiguous`, `interpreted`, `extends` (the result is no longer strict standard) or `restates` (a cited text made executable) |
| level, text, reason | what is decided, at which level, and why |
| references | quoted strings: a specification document, a page or paragraph, or a path in this repository. They are **never read at run time** |
| anchors | where the rule shows (a view, an object, a command) |

English is the source and is always present. A translation is optional per rule;
a rule without one answers in English **and says so**.

A rule that changes meaning gets a **new id** that supersedes the old one, which
becomes `superseded`: history is information. A new rule enters the registry in
the same change as the decision or the code that makes it.

The crate is a leaf with no runtime dependency (WebAssembly included): the TOML
is compiled by a build script into statics, and nothing is parsed when a program
runs. The loader refuses a malformed rule with a named error (file, rule, field).

## Reading them

```sh
energyml rules --house                         # all of them
energyml rules --house --theme colour          # one theme
energyml rules --house --id <rule-id>
energyml rules --house --anchor <anchor>       # the rules that apply where something shows
energyml rules --house --json
```

Themes today include attachments, colour, inheritance of properties, series and
tables. TODO(verify): themes at release time.

## The full list

The complete, current list is **generated from the registry** and will be added
to this page by the documentation build; it is deliberately not copied by hand
here, because a hand-copied list is out of date the day a rule changes.
TODO: generated `docs/house-rules-list.md`.

## Why this matters

A data format that says nothing about a case still gets a producer's answer, and
a tool that quietly picks another one yields results nobody can explain.
Registering the choice, with its reason and its status, lets you disagree with it
explicitly. See also [ADR 0005](adr/0005-refuse-rather-than-draw.md).
