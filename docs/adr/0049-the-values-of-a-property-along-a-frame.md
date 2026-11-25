# 0049 - The values of a property along a wellbore frame are read once, by `frame_values`

Status: accepted

## Context

*What is this property worth along this well?* was answered three times, each its own way:

- `wellbore_table` tabulated a frame, one row per node or per interval, and compared the indexable element with
  `intervals` alone. A frame whose properties say `intervals from datum` was written as a table of nodes, every one of
  its properties refused with a note (issue #34 of the public repository). `well_strata` took `intervals` and
  `intervals from datum`; `element_count` took both; the sheet reader, the matrix reader and
  the kinship of properties each spelled their own list.
- The two readers read a category table differently. `wellbore_table` named a row by the table's key (`row_named_by`)
  and wrote the last column that is not a key; `well_strata` built its own map from key to row, parsing a text key as a
  number, and joined every column that is not a key. A code named one row in one reader and none in the other.
- They did not refuse alike: `wellbore_table` read the first patch without a word, `well_strata` read it with a note,
  neither refused a property that holds several values per element.

The readers that come next (a drawing coloured by any property of a well, the limits between two runs of values, the
contacts a property gives) need the same values, the row of the table each code names, and the value at an MD that is
not a node. Written in each of them, they would be a fourth, a fifth and a sixth reading.

## Decision

- **`IndexableElement::is_frame_interval` is the one door for the intervals of a frame**: `intervals` and
  `intervals from datum`. They hold `NodeCount - 1` values, one per node-to-node span; `from datum` says how the extents
  are *expressed*, not how many there are, and `property::element_count` already counts the three alike. `interval
  edges` is not one of them (it counts the edges between the intervals of a column-layer grid, and is refused on a
  frame). Every reader that asked the question another way asks the door.
- **`energyml_domain::frame_values` is the one reading of a property along a frame**, beside `wellbore_table` and
  `well_strata`, on top of `property::read`, `Patch::read` and `table::Lookup`:
  - `of(frame, property, source)` (and `node_mds` + `of_with_mds` for a caller that reads many properties of one frame;
    `of_given` for a property the caller already found hanging on the frame, as `wellbore_table::hangs_on` does)
    gives a `FrameValues`: the **support** (`Nodes { md }` or `Intervals { top, base }`), the **values**
    (`Vec<Option<Value>>`, a number or a code, `None` for the file's absence), the unit, the kind, the range of the values
    that are not absent, and, for a property that names a table, `Categories`: the raw codes, the row each names
    (`row_of`), the whole row with each column's title, kind, unit and value (`row`, `row_at`), and a label that is
    *a column among others* (`label_column`: the value column of kind `code`, else the only text column, none guessed).
  - **A code names its row by the key, by the rank when the table has no key** (`table::row_named_by`, the one rule);
    a key column of text that reads as numbers names its rows by those numbers after the exact match, which is how
    `well_strata` always read it.
  - **It refuses by name** (`Refusal`): a number of values that is not the number of nodes or of intervals, several
    patches, several values per element (a vector: which component?), `RealizationIndices` (which realization?), a
    property that hangs on another representation or on neither the nodes nor the intervals, a table that is not there
    or cannot be read. A property with a time index is **not** refused: it is one instant of a series, `time` says which,
    and a property that wrote every instant in one array is caught by the count.
  - **`FrameValues::at_md(md, side, method)`** gives the value at any MD, through `along_md`: a continuous value is
    interpolated between two nodes (`linear` unless another registered method is named), **never over a hole** (an absent
    neighbour, a gap wider than `GapRule`); a discrete value is held at its node and refused between two; on intervals
    the interval that holds the MD, and on the node between two the side asked (`Side::Below` is the interval the node
    opens, the rule of `well_strata`; the deepest node closes the last interval).
  - **`FrameValues::derive(column)`** gives a numeric column of the table as a continuous property of the same support,
    in the unit and of the kind of the column, nothing stored. It refuses a column that is not numeric, a column of
    several values per row, a code with no row, a table that repeats a key, and a function table
    (`DoubleTableLookup`), each by name.
  - **`frame_values::carry`** puts the values of the frames of a wellbore on another representation of the same wellbore
    by MD, with the rule of `strata_by_md::carry` (`comparable`: one trajectory, or one datum, one MD domain, one unit up
    to a conversion; a frame that fails it is set aside by name, none left is a refusal). Nothing is averaged; an MD no
    frame holds is `Carry::Outside`, an MD where a frame says nothing is `Carry::Absent` (which disagrees with nothing),
    two frames naming two values are a `Carry::Disagreement`.
- **`wellbore_table::of_frame` and `well_strata::of_frame` read through it.** Their outputs are those they had; what
  changes is listed under the consequences.
- `table::Lookup::read_described` reads a table with what each column says of itself (`ColumnMeta`); `Lookup::read` is
  that without the descriptions, so a table is read by one function.

## Alternatives considered

- **Widening `wellbore_table` alone** (compare with the three elements and stop). It fixes #34 and leaves the two
  readers of a table, the two row rules and the missing refusals; the next reader would copy one of them.
- **Putting the reading in `well_strata`**, which already reads a category property on a frame. It is about
  stratigraphic units: it makes units, colours and sections, and a reader of a continuous property would pull all that in
  for nothing. The unit-specific part stays there, over the door.
- **One label per category, taken by a single rule, for both readers.** `wellbore_table` writes one column per column of
  the table that is not a key and `well_strata` joins them with ` - ` into the unit's name; either rule used by the other
  changes its output, and neither is the rule `label_column` proposes. The readers keep what they write and read the row
  from the door; `label_column` is for the readers that need one word.
- **Refusing a property with a time index.** It would make every instant of a series unreadable along a well; the instant
  is exposed instead.

## Consequences

- `intervals from datum` is a pass: a frame whose properties say so tabulates one row per interval, with a top and a base,
  exactly like one whose properties say `intervals` (#34). The property offer of the image track takes `intervals from datum`.
- A property with several patches, several values per element or `RealizationIndices` is refused by `wellbore_table` and
  `well_strata` with a note or a refusal that says so; both used to read the first patch (`well_strata` with a note) and to
  fail on the count for a vector. A value that is not finite is an absence in both (`well_strata` kept an infinite value
  and found no row for it).
- A text key that reads as a number names its row in `wellbore_table` as it did in `well_strata`.
- A reader that wants the values of a property along a well, the row of its table, the value at an MD or a table column as
  a property writes none of it.
- Not done here: the colour a table gives its categories, the limits between two runs of values and their placement, and
  the contacts a property gives. They read through this module.
