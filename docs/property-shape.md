# The shape of a property

A property is numbers attached to the elements of a representation: one value, or
a vector or a tensor, per node, per cell, per triangle. Reading the numbers is
done ([Value arrays](library.md)); this page is about what comes before and
after: **what shape the numbers have**, how to read **part** of them, and how to
**describe** a property without loading it whole. Everything here is in
`energyml-domain`, in pure Rust, and builds for WebAssembly.

The library refuses rather than guesses. A shape that does not add up, a block
the store did not honour, a pick that does not fit: each is a named refusal, never
an empty array that looks like a property with no values.

## The order of the dimensions

RESQML TRG 2.2 §7.1 (p. 222): *"First (left) given dimension is slowest and last
(right) given dimension is fastest."* The values of a property are structured, top
is slowest, as `[RealizationCount] [TimeCount] [IndexableElementCount]
[ValueCountPerIndexableElement]` (§7, figure p. 221), and the Usage Guide 2.2
§6.6.3 (p. 65) gives the same four dimensions in the same order. The Usage Guide
§4.2.2 (p. 39) adds the rule that makes a flat store and an N-dimensional one the
same data: *"when viewed as an equivalent 1D array, the HDF5 data storage ordering
and the RESQML index ordering are identical."* Flattening consecutive dimensions
does not change the order of the values.

`property::shape::ElementShape` is the shape of the components of **one** element,
slowest first, with their labels (`LabelPerComponent`, linearised like the values).
A scalar is `[1]`. `ravel` and `unravel` are the only multi-index arithmetic of the
library; a zero dimension is refused.

`PropertyShape` is the whole shape, `[realizations][time][elements...][components...]`.
The elements are `ElementAxes::Ordered` when the guide states an order (an IJK grid
is `[nk, nj, ni]`, a frame's nodes `[n]`) and `Unordered` otherwise (triangles,
scattered points). `PropertyShape::layout(stored)` confronts the shape the document
declares with the one the store holds, and accepts only a **regrouping** of
consecutive dimensions: `[n, 3, 2]`, `[n, 6]` and `[n * 6]` are the same property,
`[6, n]` is not.

### Time

A property dated by a series (the 2.2 `TimeOrIntervalSeries`) has a time dimension;
one dated by a single index (2.0.1 `TimeIndex`) has none, and the Usage Guide says
that inherited index *"should be ignored if also present"*. With `UseInterval`, a
series of `n` instants has `n - 1` steps (the library's reading of an element the
EML TRG 2.3 leaves without text, p. 296; see
[ADR 0029](adr/0029-convention-b-links-by-qualifier-facet.md)). A time dimension
that contradicts the series is `TimeCountMismatch`.

## Choosing a component

`ComponentPick` is the same choice for every view: `All`, `At` (one component by its
multi-index), `Along { axis, at }` (every component along an axis, the others
fixed), `Reduce` (one value per element), and `AlongReduced`. `check` says before
anything is read whether the pick fits the shape; `width` and `positions` say what
it keeps; `select` applies it to one element's values. A `NaN` chosen by `All`, `At`
or `Along` stays `NaN`.

```rust,ignore
use energyml_domain::property::shape::{ComponentPick, Reduction};

// The plane "second component of the first dimension fixed", every element.
let plane = ComponentPick::Along { axis: 0, at: vec![0, 1] };
```

### Reducing a dimension

`AlongReduced { axis, at, reduced, by }` shows the dimension `axis`, fixes the
dimensions it does not name at `at`, and **reduces** the listed ones: a projection
of a matrix onto one of its axes. `by` is a `Reduction` (`Mean`, `Min`, `Max`, and
`Norm` for a whole vector only), whose ids (`"mean"`, `"min"`, `"max"`, `"norm"`)
are stable and are what a view document writes. `Reduction::apply` is the one
arithmetic: only finite values take part, and a mean over several reduced
dimensions is the mean of the finite values of the **whole** sub-block, not a mean
of means (see [ADR 0042](adr/0042-what-a-property-says-of-itself.md)). The
drawing crate has its own reducers for rows of pixels; the two are written twice on
purpose, because drawing knows nothing of the domain
([ADR 0028](adr/0028-one-drawing-for-every-view.md)).

`ComponentPick::narrowed` says which sub-block of the components a pick reads, so
that a store which keeps each component dimension apart can be read for that block
only. For a matrix of 301 by 81 by 81 values per element, one plane is 24 381 values
rather than 1 974 861.

## Reading a block

`values::read_block` reads a block by the **stored** dimensions, and a source that
returns another number of values than the block holds is refused by name
(`values::stored_dims` gives the stored shape from the header, reading no value).
`values::element_block` is the block of a run of elements; `values::slab_block` is
the block of some slabs of the first free dimension, with the slower ones fixed (a
realization, a time step), and, when the store allows, only the components a
narrowed pick reads. They are the one block arithmetic: nothing else in the library
computes a `start` and a `count`.

## What each dimension stands for

`component_axes` says what each dimension of a vector property stands for: labels,
a paired property, a table linked by the convention B (a `qualifier` facet whose
value is a full ETP uri, `property::qualifier_link`), a declared link, a WITSML
`AxisDefinition`, or ranks when nothing says. It **proposes** a table
(`candidate_tables`) and never applies it. `series::matrix` reads a matrix per
element along a well (`MatrixChannel`) and one plane of it (`read_plane`), in part
when the store allows.

## Siblings and shape candidates

`PackageIndex::siblings` finds the properties that go together (the horizontal and
vertical sister, the time steps of one property, a pair radius and angle), with a
`Kinship` that says why. `PackageIndex::shape_candidates` gives the facts a
detection needs without reading any array.

## Describing a property

`property::chunks::PropertyChunks` presents a property as the `ValueChunks` that
`stats::describe::Describer` reads, slab by slab, **without loading the array**:

```rust,ignore
use energyml_domain::property::chunks::{ChunkSelection, PropertyChunks};
use energyml_domain::stats::describe::{Describer, Plan};
use energyml_domain::stats::modes::StatMethods;

let property = property::read(object)?;
let chunks = PropertyChunks::new(&uri, &property, elements, instants, None,
                                 &ChunkSelection::default(), &source)?;
let mut describer = Describer::new(&chunks, Plan::default(), StatMethods::builtin())?;
while !describer.progress().is_done() {
    describer.step(&chunks, 16)?;          // a few slabs, then ask whether to go on
}
let description = describer.finish()?;
```

The slowest free dimension is time when every step is kept (`TimeChoice::All`) and
the first dimension of the elements otherwise. `ChunkSelection` carries the pick,
the time step (`TimeChoice::Step`) and the realization, which **must** be chosen
when the values have one dimension of them. A pick that reads one plane reads only
that plane from a store that keeps its component dimensions apart; a flat store is
read whole, and `reads_narrowed()` says which happened.

The `NullValue` of an integer array is left to the describer, which counts the
nulls and leaves them out, unless the pick reduces: the null is then replaced by
`NaN` before the reduction, and `nulls_before_reduction()` says how many were met.
A categorical property (it names a lookup table), a boolean or text one, several
patches, and a property that declares several values per element and holds one
under a pick other than `All`, are refused by name (`ChunksRefusal`).

`keep_ordered_axes` drops from a description what it says of elements that have no
"along" (`AxisId::Element { ordered: false, .. }`): no variation is measured across
triangles.

### A property and its sister

`PairWalk` reads a property and its sister by the **same** slab into the
`PairAccumulator` of `stats::pairs`: the density, the ratio `y / x`, the share that
satisfies a rule the caller gives, the correlation. Two properties that do not read
as the same dimensions are refused, never aligned silently. The kinship of the two
is not checked: the caller chose them. What it counts is a figure; that a share is a
warning on a directional ratio is the rule `house/property/directional-ratio`
(see [Statistics, scales and measured depth](statistics-and-depth.md)).

## What a property says of itself

`property::observe::observations` lists, typed and without sentence, what the
**file** declares about a property, reading no value:

- `Shape`: the declared shape and how the store regroups it; `StoredDisagrees`, or
  `DeclaredCountWrong` when several values per element are declared and one held;
- `ExtensionFacet`, `FacetValueOfOtherKind`, `FacetKindRepeated`: a producer's own
  facet value (EML TRG 2.3 §11.2 p. 284), a standard value under another kind than
  the schema documents for it (the table `FACET_VALUE_KINDS`, copied from the
  documentation of the schema and confronted with it by a test; not applied to a
  2.0.1 value, which is free text), several facets of one kind (§11.11 p. 294);
- `Unit`: the unit against the quantity class of the kind: `Checked`, `Outside`, or
  `NotCheckable` with the reason (no unit, no kind, a kind that does not resolve, no
  class, a class this build does not convert). A class this build does not convert is
  never reported as checked;
- `Kind`: another issue of the kind (abstract, deprecated);
- `Synthetic`: the `synthetic` mark of [ADR 0019](adr/0019-synthetic-data-is-flagged-and-sourced.md),
  on the property or on its supporting representation. Nothing is said of an object
  without the mark.

These sit beside the observations of the values (`stats::describe::Observation`),
and no fact is written twice. The decisions of interpretation are in
[ADR 0042](adr/0042-what-a-property-says-of-itself.md).
