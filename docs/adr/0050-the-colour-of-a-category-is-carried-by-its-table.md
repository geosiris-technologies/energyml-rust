# 0050 - The colour of a category is carried by its table

Status: accepted. Extends [ADR-0009](0009-properties-tables-colours.md).

## Context

A categorical or discrete property stores codes, and the table its `CategoryLookup` names says what each code is. Some
producers also say **what colour each category is**, and they say it on the table: a `ColorInformation` of a
`GraphicalInformationSet` whose `TargetObject` is the table, whose `ColorMap` is a `DiscreteColorMap` whose entries are
indexed by the **keys** of the table, and whose `ValueVectorIndex` names a **column** of the table (in the packages the
issue reports, `1`: the label column, counting the key column from 0).

The cascade of ADR-0009 knew the property, its kind, the representation, the interpretation and the feature. It never
asked the table, so the categories of a translated package had no colour in a drawing and `energyml colors` answered
*no colour at any level* (issue #35 of the public repository). The standard defines neither the precedence of graphical
information nor this use of `ValueVectorIndex` (which the schema defines as an index into the data: for a vector
property, its component).

## Decision

- **A rung between the property and its kind.** For a category property the cascade asks, after the property's own
  `ColorInformation` and before its kind's: a `ColorInformation` targeting the table. The table is about *these*
  categories, the kind about every property of that sort. The level is `Level::CategoryTable`.
  `Cascade::table_decision(table, objects)` asks it alone; `Cascade::resolve_category(request, table, objects)` asks the
  three in order and falls back on `resolve`, which is unchanged.
- **A category is its key: it is sampled by its exact key, wherever its map is found.** `ColorMap::sample_key(key)` finds the
  entry whose index is exactly the key. The nearest-lower rule of a discrete map is for measures; applied to a key it would
  give a category the colour of another. This holds for a discrete map on the table, on the property or on its kind alike,
  for the units of a well (`well_strata`) and for the views (`value_colours` with `Reading::Categorical`); `ColorMap::sample`
  stays for the continuous maps and for a measure. A key with no entry has **no colour from the map**: `well_strata` says so
  in its notes and asks the unit's own colour; a view takes the default palette of the reading for that key and its source
  says it (`ColourSource::FileAndPalette`).
- **`ValueVectorIndex` on a table is a column, counted from 0 with the key column or columns first**, the order of
  `table::Table`, which is the same for the two forms: `ColumnBasedTable` (2.2) and `StringTableLookup` (a key and a
  value, 2.0.1). Absent, it means the table as a whole.
- **An index that cannot be placed is refused by name**, and the colour is not guessed: a column the table does not have,
  a table keyed on several columns (a map indexes a row by one key), a column asked of a table the package does not
  hold. The refusal is the error of `table_decision`; `well_strata` says it in its notes and falls back on the colour the
  units themselves carry, `energyml colors` and the views report it as that object's refusal.
- **One door.** `well_strata` (the units of a well: the legend and the column) and the package's own colours ask it
  through `Cascade::table_decision`; in the cache the walk of a property, `decide_in`, asks it once for every caller:
  `Cache::decision` (the 3D scene), `of_package` and `decisions_of` (`energyml colors`, the views' colour door). A category
  has the same colour in the scene, in a section and in the listing.

## Consequences

- The categories of a package that writes its colours on the table are coloured, in the strata column, the legend and the
  views; nothing changes for a package that does not.
- `ColourSource` gains a variant, `FileAndPalette`, said in the legend of a view.
- `Level` gains a variant: a program that matched it exhaustively (the command line did) names the new level.
- Tests are written from the schema and the guides on objects built in the test; the real packages are looked at, never
  asserted.
