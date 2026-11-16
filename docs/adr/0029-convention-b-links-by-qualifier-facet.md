# 0029 - A link to a table is one `qualifier` facet holding the uri of the table

Status: accepted

## Context

Two things need to point at a table the standard gives no field to point at:

- a **column** of a table that holds the key of another table, or the rank of one
  of its rows;
- a **property** whose components have axes (a length from -40 to 40 m, sixteen
  angles) that live in a table.

RESQML TRG 2.2 section 7.1 (p. 222) gives a property labels per component and
nothing else; EML TRG 2.3 has no link from a column to a table. Left to each
reader, the link would be read as many ways as there are readers, and each way
is a guess about what a producer meant.

What the standard does say, and what a link can rest on:

- EML TRG 2.3 section 11.11 `PropertyKindFacet` (p. 294): *"Don't use several
  facets of the same kind for the same property."* One `qualifier` facet can
  carry one link; a second beside it cannot be a second link.
- EML TRG 2.3 section 11.3 `FacetKind` (p. 285): `qualifier` is *"used to capture
  any other context not covered by the other facet types listed here"*. An
  ordinary qualifier (`cumulative`, `measured`) is context, not a link.
- EML TRG 2.3 section 11.2 `FacetExt` (p. 284): the list of values is open, so
  a uri is a legal value.

## Decision

- **Convention B.** A column, or a property, links to a table by **one** facet of
  kind `qualifier` whose value is the **full ETP uri** of that table. The
  convention is this library's own, written down here.
- **One reading.** `property::qualifier_link(facets)` is the only function that
  reads it. It returns the uri, `Ok(None)` when no `qualifier` facet holds a uri
  (an ordinary qualifier is left alone), and refuses by name: a uri beside another
  `qualifier` facet is `Several { count }` (the standard allows one facet of a
  kind), a value that starts like an ETP uri and is not the uri of an object
  (a dataspace uri, a truncated one) is `MalformedUri`. It does not say whether
  the object named is a table: each caller checks that against what it needs. The
  table side (column links, `LinkReaders`) and the property side (axes) both call
  it; neither reads a facet list a second way.
- **The axes of a property.** The table the facet names holds **exactly one key
  column**, and one **value column per dimension of the components, in the order
  of the document, the slowest dimension first**. That order is an
  interpretation: the standard states none for a table. A table has one row count
  for all its columns, so a shape whose dimensions differ (`[3, 2]`) does not fit
  a table and is refused by name. A table with another number of value columns,
  or of key columns, is refused by name.
- **A declared source comes before the file.** What the caller declares (a table,
  a paired property, a WITSML channel) is tried before what the file says
  (convention B, `LabelPerComponent`); the first source to answer for a dimension
  wins, and a dimension nobody answers for has its ranks and says so. A source
  that applies and fails refuses: an axis that was declared is never silently
  replaced by ranks. A table that merely looks right is **offered**
  (`candidate_tables`) and never applied.
- **Nothing is written to the package** by this reading: a declaration the
  caller makes is a parameter, kept in a recipe or a view document.
- **`UseInterval` is read as `n - 1` steps for `n` instants.** The standard
  names the field without a text (EML TRG 2.3, p. 296, `TimeOrIntervalSeries`);
  the library has read a value per interval since before this decision
  (`TimeRef`), and the shape of a property keeps that reading.

## Consequences

- A column or a property that links by convention A (the target written in a
  description) is not read by this function; the table side reads it, with a
  warning (see the decision on table links).
- A producer that writes two `qualifier` facets, one of them a uri, gets a
  named refusal rather than a silent choice of the first.
- The order of the value columns is the one place a file written by someone else
  may mean the opposite; the refusal of unequal dimensions catches the case where
  it matters most, and a reordering is the caller's declaration.
