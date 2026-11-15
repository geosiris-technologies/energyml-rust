# 0035 - Provenance, written completely and read from four releases

Status: accepted

## Context

The library wrote provenance that did not satisfy the rule of the specification it
claims to follow. The EML technical reference guide 2.3 (section 3.1, p. 40) says
the title of an activity parameter *must have an equivalent in the activity
descriptor parameters*; yet the operations wrote parameters titled `input` that
no canonical template declared, whole numbers as doubles, and the file a recipe
step had read nowhere. A graph of provenance drawn from such a package had to guess
which parameter was an input.

Reading was the other half. Activities come in four releases (RESQML 2.0.1, EML 2.1,
2.2, 2.3), with a template that may or may not be in the package, and a role that is
stated only in the template. Some converters, such as UDAR with its NWDF tables,
keep provenance in tables outside any activity, and a drawing that ties a dataset
to its objects by a regular expression on the title looks right and is wrong.

## Decision

1. **A `DataObjectParameter` that points to an `Activity` is read as `subActivity`.**
   No class of parameter has that kind in the schemas; it is the way the library's
   recipes already write a workflow's steps.
2. **A `TimeIndexParameter` is read as `timestamp`**, for the same reason.
3. **The role of a parameter comes from the template, then from a named rule, else it
   is `Undeclared`.** A role is never guessed from a title. The one built-in rule,
   `energyml-build:input-output`, reads `input` and `output` in an activity written
   by `energyml-rust`; it exists for the packages written before this decision, and
   its id is shown beside the role it gave. Rules are a registry, the caller's first
   (ADR 0006).
4. **Every canonical template declares `input` and `inputFile`.** An `inputFile`
   parameter is a string whose value is `<slot>: <path>`, or `<path>` when no slot
   was named. A new package satisfies the rule of section 3.1.
5. **Whole numbers are written as `IntegerQuantityParameter`**, as the canonical
   template declares them (`Parameter::Integer`, a breaking change of the enum).
6. **A verbatim source is a public trait with a registry; the NWDF reader is public.**
   Its tables are recognised from column titles, one table per role (two are refused
   as `SeveralTables`, not merged), the objects of a key come from the mapping table
   and never from a title, and a key nothing resolves is shown as unresolved. When
   a process's activity is stated twice and the two differ, both are kept and the
   disagreement is signalled.
7. **A recipe draft rebuilt from a workflow is lossy, and says so.** What it cannot
   restate is refused by name.

## Consequences

- A package written from this version satisfies section 3.1 without an exception in
  the template check; the check that excepted `input` is gone.
- A package written before it still reads: its `input` parameters get their role from
  the built-in rule, with the rule's id shown, and `conformance` signals the
  parameters the template lacks. Nothing is refused for being old.
- The rule of the registry (house rules, theme `provenance`) reports what departs
  from a template; it never blocks a read.
- A reader of an activity cannot tell a written integer from a written double in a
  package of another software: a number is read by the field present, and the
  conformance reports a difference of kind against the template.
- Replaying the Lutetia model changes its templates and activities; its lock file
  moves in the same release.
- Two tables of one role (two converted boreholes in one package) are refused until
  there is a way to tell the groups apart, which is a future case, not a guess.
