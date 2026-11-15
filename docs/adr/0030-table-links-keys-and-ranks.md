# 0030 - Table links: conventions, keys and ranks

Status: accepted

The convention that makes a column a link, one `qualifier` facet holding the
uri of the target table, is [ADR 0029](0029-convention-b-links-by-qualifier-facet.md)
and is not said again here. This record is the table side of it: what a link
names, how a row is found, what is read beside the facet, and what is reported.

## Context

The specification has no column that names another table. Common TRG 2.3
sections 9.1 and 9.2 (p. 190-191) give a table columns, a `KeyColumn` that
*"makes the table a lookup table"*, and positional rows (*"Ordering matters since
it will be the same in all columns"*), and nothing that says that a value of a
column is the key of a row of another table. No property kind says it either:
`identifier` is *"a number, label or code given to an object in an attempt to
uniquely identify it"*, and `explicit index` is a token *"used as a reference or
pointer to a value stored in some data structure"*. Producers write the target
where they can: in the description of the column (the target uri and the name of
its key column), or by the title of the table in a sibling column.

Nor does the specification say how a value names a row of a lookup table, nor
that keys are unique, nor what a key is when the table has none.

## Decision

- **A link is written as in ADR 0029**: kind `identifier` when the value is the
  key of a row of the target, kind `explicit index` when it is its rank, and one
  `qualifier` facet that holds the complete ETP uri of the target table (the
  list of facet values is open, section 11.2 p. 284; `qualifier` is *"any other
  context"*, section 11.3 p. 285; one facet per kind, section 11.11 p. 294).
- **The target decides how a row is named.** A target table has **one key
  column** (a value names a row by its key) or **none** (by its rank, counted
  from 0). A table with several key columns is refused by name for every
  convention: a value is not a tuple. A table with a key column is **never** read
  by rank, so a key that finds nothing never lands on an unrelated row. The kind
  of the link column and the target can disagree; the target wins and the
  disagreement is reported. This one rule is `table::row_named_by`, and
  `house/discrete-value-in-lookup` reads a category value the same way.
- **Two other ways of declaring a link are read, with a warning.** A: the
  description of the column says `key of the table <title>: <uri>, KeyColumn
  <key>` (or its self-reference form). D: a sibling column titled `<stem>_table`
  names the target table, row by row, by title (a title that names no table of the
  package is an external reference, said as such; a title shared by two tables is
  refused, because a title is not an identity). Both are conventions of
  producers, not of the specification, and the documentation says so. When the
  facet and the description name two different tables, neither is followed and
  the disagreement is refused by name.
- **A column of object uris is not a link to a row.** A column whose every
  non-empty value is a complete ETP uri (convention E) names objects, not rows:
  its values are read as object chips, and a uri that names nothing the package
  holds is reported.
- **Aliases are refused.** A name held in an alias is not an identity
  (`Identifier` is a 64-character string, not a reference).
- **An empty string is a value, and names no row.** A cell has four states: a
  value, an absence (the array's `NullValue`, `NaN`), an empty string and a
  failure to read. Absence and emptiness are not the same thing, and none of the
  four is turned into another.
- **Keys compare exactly.** Two numbers are the same key when they are equal
  (`-0.0` is `0.0`; `NaN` is never a key). A tolerance is not transitive and
  cannot back an index.
- **A property names the table of its axes the same way** (ADR 0029).

## Consequences

- The rules `house/table/link-resolves`, `house/table/link-target-key-unique`,
  `house/table/link-declared-by-facet` and `house/table/uri-value-resolves` are
  checks of `energyml check`, with their findings; `house/table/row-rank-without-key`
  is the rule of reading the others cite, with no finding of its own. A link
  whose target is not in the package, or that cannot be followed, is
  *uncheckable*, never a pass.
- `house/discrete-value-in-lookup` keeps its id: its text now says that a value
  names a row by the key, or by rank when the table has none. A table without a
  key column, which the check used to leave unverifiable, is now verified.
- A facet is turned into a reference, which is the cost of ADR 0029: a reader of
  the package that ignores the library sees a qualifier it does not understand.
- Writers (`energyml-build`) are to write convention B, and derive any
  description (convention A) from it, so the two never disagree.
- Whether to propose the conventions to Energistics is open; until they are
  adopted they are ours, and the reports say so.
