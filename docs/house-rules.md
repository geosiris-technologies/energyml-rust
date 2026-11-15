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

Themes today include attachments, colour, inheritance of properties, series,
statistics, tables, the well and directional surveys (`energyml rules --house` lists the eight themes of this release).

### Statistics, scales and gaps

Along the measured depth, a gap wider than 3 median steps is never bridged, and a jump of the value wider than 10 median steps of its variation is signalled as a seam. A logarithmic scale is proposed when the values span 3 decades, and automatic bounds are the 1st and 99th percentiles. Read them with `energyml rules --house --theme stats`; the decisions are in [ADR 0027](adr/0027-gaps-seams-scales-and-quantiles.md).

### Around the well

Where a trajectory is evaluated, the direction at a knot of a linear line is that of the interval that ends there, and a hole under 1 degree of inclination has no high side, so its frame is refused at that node by name. A true vertical depth along a well is counted below the MD datum, and a convention the file does not state (the sense of angles, the axes of a per-node matrix) is declared by the caller, never guessed. Read them with `energyml rules --house --theme well`; the decisions are in [ADR 0031](adr/0031-tangent-at-a-knot.md), [ADR 0032](adr/0032-hole-frame-in-a-direct-placed-frame.md), [ADR 0033](adr/0033-tvd-below-the-md-datum.md) and [ADR 0041](adr/0041-declared-conventions-and-the-vertical-without-crs.md).

### Tables and links

A column links to another table by one `qualifier` facet that holds the complete uri of the target (convention B, [ADR 0029](adr/0029-convention-b-links-by-qualifier-facet.md)); the target table has one key column, by which a value names a row, or none, and then a value names a row by its rank from 0. A link that leads nowhere is an error, a key held by two rows, a declaration made in a description or a title, and a uri that names no object of the package are warnings, and a link that cannot be checked is said to be uncheckable, never passed. Read them with `energyml rules --house --theme table`; the decisions are in [ADR 0030](adr/0030-table-links-keys-and-ranks.md) and the way to use them in [Tables, keys and links](tables.md).

### Directional surveys

A survey that states no north is placed only with a north the caller declares, which is carried in the result and never written into the package, and a declaration never overrules a north the file states. A survey in true or magnetic north is turned to grid north by the convergence and the declination the file states, and a station left out as rejected is left out only when the caller declares so. A RESQML 2.0.1 deviation survey is read from the north of its projected CRS. Read them with `energyml rules --house --theme survey`; the decisions are in [ADR 0034](adr/0034-survey-north-and-stations.md).

## The full list

The complete, current list is **generated from the registry**: run
`energyml rules --house` (add `--json` for a machine-readable form). It is
deliberately not copied by hand here, because a hand-copied list is out of date
the day a rule changes.

## Why this matters

A data format that says nothing about a case still gets a producer's answer, and
a tool that quietly picks another one yields results nobody can explain.
Registering the choice, with its reason and its status, lets you disagree with it
explicitly. See also [ADR 0005](adr/0005-refuse-rather-than-draw.md).
