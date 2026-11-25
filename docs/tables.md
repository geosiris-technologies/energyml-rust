# Tables, keys and links

A table is columns of values: a category table (facies 3 is *sandstone*), a
relative permeability table, the tables an application keeps beside a package
(indexes, activities, files). This page is about what a cell holds, how a value
names a row, how a column links to another table, and what the library says about
the links, the rows and the depths of a table. Everything here is in
`energyml-domain`, in pure Rust, and builds for WebAssembly.

The library refuses rather than guesses. A link that leads nowhere, a table it
cannot read, a key it cannot use: each is a named refusal or a finding, never an
empty cell that looks like a table with nothing to show.

## What a cell holds

A cell has **four states**, and none of them is turned into another:

| State | What it is |
|---|---|
| a value | a number, a boolean, a non-empty text |
| absent | an integer equal to the `NullValue` of its array, or a `NaN` |
| empty | an empty string: a **value**, not an absence |
| unreadable | the column could not be read; the refusal says why |

An empty string is a value because a `StringXmlArray` has no `NullValue`: nothing
in the file says that `""` means *nothing*. It sorts as `""`. It names no row when
it sits in a link column.

`ValueCountPerRow` is the number of values of a row (common TRG 2.3 §9.1 p. 190:
if it is greater than 1, the fastest dimension of the array is this value; a
column of 10 rows of 3 is `[10, 3]`). A column of 7 values with a count of 3 is
refused, naming the column, rather than cut to 2 rows and a half. A row of such a
column is a vector of its values, where each component can be absent.

```rust,ignore
use energyml_domain::table::{CellState, Table};

let table = Table::read(object)?;
let columns = table.read_cells(&arrays);          // one entry per column
let cells = columns[1].as_ref().expect("readable");
match cells.cell(0) {
    Some(CellState::Value(cell)) => println!("{cell}"),
    Some(CellState::Absent) => println!("no value"),
    _ => {}
}
```

Rows are positional: row *i* is index *i* in every column (common TRG 2.3 §9.1:
*"Ordering matters since it will be the same in all columns"*). Columns that do
not hold the same number of rows are reported with the rows they share, never
truncated in silence.

## Which row a value names: the key, else the rank

A `KeyColumn` makes a table a lookup table. The standard does not say how a value
names a row; the library reads it one way, in one place, `row_named_by`:

- a table with **one key column**: a value names the row whose key it equals;
- a table with **no key column**: a value names a row by its **rank, from 0**;
- a table with **several key columns**: a value names no row (it is not a tuple),
  and the refusal says so.

A table with a key column is never read by rank: a key that finds nothing would
land on an unrelated row. Numbers compare exactly (`-0.0` is `0.0`, `NaN` is never
a key), and an empty key names nothing. When the same key is held by several rows,
the first is used and the duplicates are reported.

```rust,ignore
use energyml_domain::table::{Cell, Lookup, RowMatch, row_named_by};

let lookup = Lookup::read(object, &arrays)?;
let index = lookup.key_index();                   // built once per table
assert_eq!(row_named_by(&index, &Cell::Number(20.0))?, RowMatch::One(1));
```

The category check of a property (`house/discrete-value-in-lookup`) reads its
values the same way.

A wellbore table (`wellbore_table::of_frame`) writes the **label** of a discrete
property, never its code. A code no row of the table names, and a code equal to
the array's `NullValue`, are empty cells; the first are counted in the table's
notes. A property whose lookup is missing or unreadable is left out with a note
that names the reason, rather than written as codes under a label's heading.

A table is **one row per interval** as soon as one of the frame's properties hangs on
the intervals of the frame: `intervals` and `intervals from datum` are one
thing (`IndexableElement::is_frame_interval`).

### The values of a property along a frame

`frame_values::of(frame, property, &arrays)` is the one reading behind that table and
behind the stratigraphy of a category property
([ADR 0049](adr/0049-the-values-of-a-property-along-a-frame.md)). It gives the
support (the nodes, or the intervals with their top and base), the values (a number,
a code, or `None` for the file's absence), the unit, the range, and, for a property that
names a table, the **codes as stored** and the row each names - every column of the
row with its title, kind, unit and value:

```rust,ignore
use energyml_domain::frame_values::{self, Side};

let read = frame_values::of(&frame, &property, &arrays)?;
let row = read.row_at(3);                              // the row the code of interval 3 names
let at = read.at_md(1234.5, Side::Below, "linear")?;   // the value at any MD, never across a hole
let thickness = read.derive(2)?;                       // column 2 of the table, as a property
```

A count that is not the number of nodes or intervals, several patches, several values
per element, `RealizationIndices` and a table that is missing are refused by name.

The findings of `energyml check` are filed under the object's **uri**: the report
items (`SchemaIssue`, `PropertyIssue`, `CountIssue`, `RuleFinding`) carry the `uri`
beside the `uuid`, so two versions of one object are told apart.

## Links between tables

The specification has no column that names another table. The library reads one
convention, and two that producers use, and says which one it read
([ADR 0030](adr/0030-table-links-keys-and-ranks.md)).

**B, one qualifier facet** ([ADR 0029](adr/0029-convention-b-links-by-qualifier-facet.md)).
The column has the kind `identifier` (its values are keys of the target) or
`explicit index` (they are ranks), and one `qualifier` facet whose value is the
complete ETP uri of the target table:

```xml
<eml:Column>
  <eml:Title>index_uid</eml:Title>
  <eml:ValueCountPerRow>1</eml:ValueCountPerRow>
  <eml:PropertyKind>
    <eml:Uuid>311dcaba-6512-4144-aa7c-79a9de19b4ab</eml:Uuid>
    <eml:QualifiedType>eml23.PropertyKind</eml:QualifiedType>
    <eml:Title>identifier</eml:Title>
  </eml:PropertyKind>
  <eml:Values xsi:type="eml:StringXmlArray">
    <eml:Values>i1</eml:Values>
    <eml:Values>i2</eml:Values>
    <eml:Values>i1</eml:Values>
  </eml:Values>
  <eml:Facet>
    <eml:Facet>eml:///eml23.ColumnBasedTable(7179e925-fba7-50be-9032-5462a75484da)</eml:Facet>
    <eml:Kind>qualifier</eml:Kind>
  </eml:Facet>
</eml:Column>
```

The target is the one that decides how a row is named: its one key column, or its
ranks when it has none. If the kind and the target disagree, the target wins and
the disagreement is reported. A `qualifier` facet that is not a uri (`cumulative`)
is not a link.

**A, the description.** `key of the table <title>: <uri>, KeyColumn <key>` (or
`key of this table (self reference): <uri>, KeyColumn <key>`). Read, followed and
reported as a convention of producers. Text that starts like this and does not
follow the form is refused, not guessed.

**D, a title in a sibling column.** A column `source_uid` with a column
`source_table` that names, row by row, the table of each value. Read, followed and
reported: a title is not an identity. A title that names no table of the package is
an external reference; one shared by two tables is refused.

**E, uris as values.** A column whose every non-empty value is a complete ETP uri
names objects, not rows. Its values are *object chips* (`source::object_chip`): the
object's type, its title when the package holds it, or the fact that it does not.
A uri of another dataspace is not looked up.

When the facet and the description name two different tables, neither is followed
and the refusal names both. The conventions are tried in order, the caller's first
(`LinkReaders::push`), and one can be dropped (`LinkReaders::without`).

```rust,ignore
use energyml_domain::table::{LinkReaders, TableSheet, resolve_kinds};
use energyml_domain::property_kind::Resolver;

let sheet = TableSheet::of(object)?;              // no array is read
let kinds = resolve_kinds(&sheet, &Resolver::new(&objects));
for (column, declaration) in LinkReaders::builtin().declarations(&sheet, &kinds) {
    println!("{column}: {declaration:?}");
}
```

## Writing a link

The library reads convention B; the `table-link` operation of
[Creating objects](creating-objects.md) writes it, on an explicit gesture and never as a
side effect. It takes the object that holds the link (a table whose column links, or a
property whose component axes the target holds) and the target table, and writes what the
reader above reads first:

- **The facet.** One `qualifier` facet whose value is the **complete ETP uri** of the
  target, its version included when it has one, never a bare uuid. A column that already
  has a `qualifier` facet of the same value has nothing to write; one of another value is
  refused, because a column links to one table.
- **By key or by rank.** `by = key` needs the target to have exactly one key column and the
  column to have the kind `identifier`; `by = rank` takes a column of kind `explicit index`
  or `classification index`, and a target with no key column is accepted. The kind is
  checked with the same rule and the same constants as the reader, and **never changed
  silently**: a column of another kind is refused, and the refusal says to set the kind
  first with `edit-object`.
- **The description.** For a link by key, when the column has no description, the
  operation also writes convention A in the exact grammar the reader accepts
  (`key of the table <title>: <uri>, KeyColumn <key>`, or `key of this table (self
  reference): ...` when the target is the table itself, as for a tree), for a human
  reader. A description that is already there is kept, and refused when it names another
  target than the facet. A link by rank has no form A and gets no description.
- **A property.** For a property, no column is named; the facet is written on the
  property itself.

The write is the one of `edit-object`, and the operation records an `Activity` with the
column, the way, and the target's uri. What was written reads back as convention B with no
warning, which is how the operation is tested against the reader.

## Checking the links

`check_links` follows every cell of every link column and counts what it finds:
rows found, dangling, ambiguous (the target key is held by several rows), external
(a title naming no table), empty, absent, and the cells that cannot be followed,
with why. `check_links_of_set` repeats it for a set of tables. `energyml check`
reports the same facts as findings, by the rules of the `table` theme
(`energyml rules --house --theme table`):

| Rule | Weight |
|---|---|
| `house/table/link-resolves` | error |
| `house/table/link-target-key-unique` | warning |
| `house/table/link-declared-by-facet` | warning |
| `house/table/uri-value-resolves` | warning |

A link whose target is not in the package, or that was given no array reader, is
reported as *uncheckable*: not having looked is not having found nothing.

## Big tables: a window

`TableWindow` keeps a sort permutation and a filter mask of 4 bytes per row, and
hands back a page of row numbers. Absent values come **last in both directions**
(an absence is neither the smallest nor the largest), an empty string sorts as a
value, and the sort is stable. A filter is typed (text, equality, range, only the
absent, only the empty, given rows, several together): one that cannot be built is
the caller's refusal, never "every row".

## How tables hold together

`table_structure` turns declarations into structure, with indexes and uris and no
labels: the schema of the tables (which links to which, and by what), the columns
that link a table to itself, the tree of its rows (on the generic `forest`, which
names each cycle and each orphan, and never hangs on a loop), the roots grouped by
a column, and the key-value shape of a table of two columns.

## Where a table or an object stands along a well

`md_extent` gives the measured-depth extent of what carries one: the interval a
trajectory, a geology object or a log declares, the minimum and maximum of a
frame's nodes, and the rows of a table that have MD columns (found by their kind,
never by their title; a row without MD can inherit the extent of the row its link
column names, and the extent says through which column). An interval written from
the larger to the smaller is kept as written and flagged. Time and true vertical
depth are on other axes and are counted apart.

`coverage` puts the extents on one picture, **one group per comparable MD
reference**. Two measured depths are comparable only when they are counted the
same way (same trajectory, same datum, same unit family): the rest are separate
groups, with the reason. A table row, which says nothing of its reference, goes to
a band of its own.

**The alignment is an assumption, and it is the person's.** An `Alignment` says
*count this item's depths as that one's are counted*. It places the item in that
group and marks it aligned; it is written on the picture and in the export, and
**never in the package**: the file did not say it.

Extents of WITSML 2.0 objects are not read yet; such an object answers that it
states no extent.
