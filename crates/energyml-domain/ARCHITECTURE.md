# energyml-domain: architecture

## Purpose and position

What every version of EnergyML has in common, implemented once by reflection over the generated types: units, coordinate reference systems, value arrays, statistics, properties and property kinds, colours, tables, business-rule checks, stratigraphy, log and time series, and the metadata specs (a JSON map of key to dotted path, which may cross references). One implementation covers RESQML 2.0.1, 2.2, WITSML, PRODML and the next release.

It sits above `energyml-storage` and below `energyml-geometry`. **No public function takes an `EpcStore`**: everything takes `&dyn ObjectSource` / `&dyn ArraySource`, so a dataspace read over a network is read like a file. Read this before adding any cross-version reader.

Related decisions: [ADR-0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR-0009](../../docs/adr/0009-properties-tables-colours.md), [ADR-0010](../../docs/adr/0010-package-index-and-caches.md), [ADR-0011](../../docs/adr/0011-tests-follow-the-specification.md), [ADR-0013](../../docs/adr/0013-witsml-channels-in-hdf5.md), [ADR-0014](../../docs/adr/0014-horizons-units-attachments-constraints.md).

## Units and coordinate systems

**Cross-version readers match the wire string, never the Rust variant** - `LengthUom::Mm` is the megametre in eml 2.0 and the millimetre from 2.1: the identifiers flipped when the generator's case disambiguation changed while the strings never moved. A variant match is a factor of 10^9 with no symptom. One unit can also have two spellings in two enums (`ft[US]` and `ftUS`); the tables accept both.

**An eml 2.3 CRS is several objects** - a compound CRS references a vertical CRS and a local engineering 2D CRS, and the offsets and EPSG code live in the latter. Reading only the compound returns a valid-looking CRS that says "unknown system". A local CRS naming a projected CRS directly is read too.

**Under a compound, the axis governs and the datum places** - the vertical axis gives direction, unit and time/depth of the local z; the referenced vertical CRS gives the datum, and its own direction and unit only convert the stated origin vertical coordinate, which the schema writes "in the base vertical CRS" (CRS XSD, `OriginVerticalCoordinate`). A non-zero origin whose datum is unreadable is refused.

**Rotation sense** - areal rotation in 2.0.1 is clockwise from the projected Y axis (RESQML TRG 2.0.1, section 2.1, p. 10) and the 2.3 azimuth is negated at read, so both releases mean the same thing after reading.

**A datum written in a compound is placed through it, once** - `ReferencePointInALocalEngineeringCompoundCrs` (common TRG 2.3, section 13.25, p. 371): the compound's identity is returned *without* its placement, because the geometry has already placed the datum and applying it twice would move the well.

**Borrowed systems** - a representation without geometry takes its geometry's system from elsewhere (a frame defers to its trajectory, a local grid to its parent, a grid connection set to the grids it joins). The lenders can be several, and **if they disagree nothing is borrowed**: two grids in two systems are two systems, and taking the first would be a guess dressed as a number.

**A generic view reads, it does not round-trip** - axis order and time-versus-depth do not survive the trip back, so the generic CRS answers questions and returns `None` where a version cannot; writing stays with the version-specific types.

**Local placement is applied; reprojection is not** - `Crs::to_projected` is exact arithmetic. Projected to WGS 84 needs a geodesy library and a choice of which; `reproject` is the seam and `energyml-proj` the implementation. An approximation that silently misplaces a model is worse than an honest gap.

## Value arrays

**One array reader covers every form and family** - about twenty concrete array types across two versions, of which a third touch a file; the rest are constants, lattices, ranges, inline XML or derived from another array. Matching is on the suffix of the type name (two names, one shape). A reader that knew only the external forms would return empty arrays for the rest, which looks exactly like a property with no values.

**Widening is opt-out** - `read` gives one type per family; `read_native` returns the values as stored; `read_into` fills the caller's buffer without allocating. The two say `None` for a computed form.

**A distribution is arithmetic, and every wrong answer looks like a chart** - `stats` holds it. `NaN` is counted apart, never averaged (it is how files say "no data here"); a discrete array with few distinct integers is counted per value, never binned, and floating-point values are never treated as discrete; the last bin is closed so the maximum does not fall off the end; a series is decimated in the library with its step stated; summarising covers the whole array, not a page of it.

## Properties and property kinds

**A count is computed or refused, never assumed** - the number of values a property should hold is `element_count x product of ValueCountPerIndexableElement`, and the first factor depends on a table the schema documents for few pairs. `property::element_count` answers for the representation types it knows and refuses by naming the pair for the rest; the pairs it answers are cross-checked against a second path. For an IJK grid, nodes are `(pillars + splits) x (nk + 1 + gaps)`, equal to the naive `(ni+1)(nj+1)(nk+1)` only without fault and without K gap.

**A computed zero is a refusal, not a count** - a sum over patches adds a missing field as nothing, and `0` is indistinguishable from a representation that holds nothing. Some node counts live in an external array, not a scalar, and are unknowable without reading it, which this function does not do. The node and face counts of an unstructured grid live in its geometry, not on the object (the object is still tried first).

**A property that contradicts itself is reported, not refused** - `Property::check` returns findings; refusing would make defects invisible in the files this library exists to repair.

**A property attaches to a shape only where it lines up** - the scope comes from `IndexableElement`, the count is checked against the geometry's cell count, and a mismatch is reported by name instead of padded or truncated (padding puts values on the wrong elements). The one stated repair: a property declaring *n* values per element while holding exactly one attaches as a scalar array, with the document's count named as the wrong thing.

**The package outranks the dictionary; the dictionary outranks nothing** - a `PropertyKind` in the package is what the producer meant and wins over a same-uuid dictionary entry, with the disagreement noted. A kind found in neither is an error naming the uuid, the cached title, both places searched and the two ways out.

**A title is never an identity** - a kind resolves by title in exactly one case (a RESQML 2.0.1 standard kind, where the name is the whole meaning), says so, and reports ambiguity instead of choosing. Local kinds resolve by uuid only.

**The dictionary is embedded from `data/`, and a missing table is a build error** - `data/property_kinds.tsv` is the compacted Energistics property-kind dictionary (produced by the generator from the Energistics XML, with a `NOTICE`); `build.rs` validates and stages it, and parsing happens once, on first use. An absent or empty table fails the build naming the file: an empty dictionary looks exactly like a library with nothing to say. `set_default` replaces it for the process, `Resolver::with_dictionary` for one call.

**A rule that cannot be applied says so** - `property_kind::check` implements three rules the technical reference guides state (abstract kind used directly, deprecated kind, unit outside the kind's quantity class). The third covers only the unit families this build converts; the others come back as `QuantityClassNotCheckable` rather than passing, and `is_a_defect()` separates a limit of this build from a defect of the file.

## Colours

**A colour is found by falling back, and the answer says how far it fell and by which path** - the three technical reference guides define no precedence or inheritance for graphical information, so the order is this library's decision (recorded as `house/inherit/*` rules). Every decision carries the level it came from and the object that carried it; the walk returns the path and any ties (first in document order wins, the others are shown), so a caller can refuse an inherited colour. Nothing found is `None`: the standard defines no default colour, and inventing one draws something the file does not say.

**The inheritance order** - the object, its collections, its parents, the parents' collections (walking upwards), then the model. Collections of the model and a project are not followed, and the walk says so. A property is a shorter case: itself, its kind, the kind's parents, a collection at each stage, and nothing beyond. "Parent" means the feature-interpretation-representation hierarchy only (`RepresentedObject`, `RepresentedInterpretation`, `InterpretedFeature`) and never a reference pointing *down*: a list naming the interpretation of each part is a per-part attribution, and following it would give one representation as many parents as it has pieces. The level is read from the type reached (RESQML 2.2: "The data object represented by the representation is either an interpretation or a technical feature"), never from the step count; an unreadable type leaves the step unnamed rather than misnamed.

**The walk reads tables the index builds, not edges** - parents, collections, memberships, graphical entries by target and attachments are rows laid in the same pass as the references; adding a relation is one line where rows are taken and its mirror where they are forgotten, or an update leaves a stale row. Tests count work (steps, objects read, walks redone), never time.

**Colour is asked per target and the walk is done once per object** - five targets; each step read once for all of them; the nearest wins, and at the same step the exact target beats the whole object, never across targets.

**The walk is by full URI end to end** - a walk's identity is the index node's URI, and a graphical entry naming version 1 does not paint version 2, while one naming no version paints all. Uuid-only fields remain for a face that shows one, each with a URI sibling. Invalidation accepting a bare uuid targets every version that contradicts nothing: wider than needed, never narrower.

**The graphical entries arrive as raw XML, and that is not a bug** - the abstract type is in eml, its concrete subtypes in RESQML 2.2, and a package cannot name the classes of a package that depends on it. Colour is read off the subtree with a narrow scanner, and the typed path is kept beside it.

**`HsvColor` has no bounds in the schema, so they are checked here** - a hue of 500 would validate; it is refused by name rather than wrapped, which would be a different colour. `ValueVectorIndex` indexes the data, not the colour map.

**A property value becomes a colour on the Rust side** - the workspace ships no palette; a face asks for one RGBA per mapped element.

## Business rules and the check report

**A rule that speaks for one release says so (`exact`)** - metadata folds `RepresentedInterpretation` and `RepresentedObject` into one field on purpose, but 2.2 lets a representation name a technical feature and 2.0.1 does not; read through the synonym, a conforming 2.2 file would be reported, and someone would "fix" it. `exact` makes an object that does not declare the literal element not the rule's business.

**A cited rule is not softened to fit the vocabulary** - "Start MD is always less than the Finish MD" is strict, so a strict `<` operator exists rather than writing `<=`, which would pass the degenerate trajectory. The vocabulary cannot compare array lengths and has no disjunction (`checks` is a conjunction). A rule can look inside (`nested`: a concept at any depth, finding prefixed by the path), name a case (`notes`), and except a narrower subclass (`except`, e.g. wellbore frames and trajectories may represent a WITSML wellbore where other representations may not). A rule written for one release is verified on the other, each release with one object that respects it and one that violates it. Weight is in the declaration: an explicit error severity, a warning otherwise; an unknown word is refused so a typo does not demote an error. The parser refuses unknown keys and requires each source and target to exist in the registry.

**The check report lives here, with its arithmetic** - this is the lowest crate that sees all three sources (what storage noticed, what the schema says, what the technical guides require). What counts as an error is a decision: a reader diagnostic of warning severity is one (the package departs from the specification), a `.rels` disagreement is not (the index is not the truth, the references are). Two faces counting differently would disagree about validity. `schema_checked` and `rules_checked` exist because an empty list does not say which of "not checked" and "checked, none found" applies. Property counts are included: a value count that does not match is an error, an integer multiple (a time series written in one dataset) is a listed remark not counted as an error, and what cannot be counted is `counts_unchecked`, never silent. A well-formed reference naming a type no known schema defines is a warning.

**The report takes no store** - `Package` is four facts that an EPC and a dataspace produce the same way.

## Stratigraphy along wells

**Same well means same represented object** (by uuid), not same feature: an interpretation is relative to one trajectory, and a feature has several (planned and drilled; TRG 2.2, section 12.14, p. 311). **Measured depths are compared under the same datum, unit and MD domain** (common TRG 2.3, section 6.48, p. 134; TRG 2.2, section 12.8, p. 304), otherwise the frame is set aside by name, never shifted; if none remain the result is `Refusal::Incomparable`. A boundary belongs to the interval it opens; two frames naming two units at the same MD are a stated disagreement, with neither chosen. A frame that states nothing is not read; one that states and reads badly is. A carried paint is cut to the extent of its nodes, never stretched to the next node. Frames of a wellbore are asked for (a function called only for an object declaring nothing itself), not supplied, and a too-wide answer is never wrong because the function discards other wells' frames.

Units are identified by URI: the identity of the object a reference names, or its address if absent from the package.

## Time scale chart

The geologic time-scale concepts and bounds are embedded from `data/geolts/` and compacted at build time by the same reader a fresh export goes through. Ages carry a decimal comma, which read as a dotted number would vanish without noise; a division can name several broader concepts (its parent is the deepest level above its own); and a code is not an identity (one code names several divisions), so lookup takes the code and the label and several answers are a refusal.

## Series

Two readers, one type: `channel_set` (WITSML rows) and `time_series` (PRODML typed samples) have nothing in common and both return a `Series` (an index, channels, a quality per sample); `to_json` is the single conversion so every face writes the same JSON. A sample is good, absent or flagged: a PRODML `status` such as frozen is not good (TimeSeriesData XSD), a WITSML `null` is absent. Decimation never connects across a gap and never averages (first, min, max, last per bucket, so peaks survive). The row layout is inferred, not quoted from a specification, and is checked against the writing side on values the test supplies. A series attaches to a well by shared name resolution; several answers are a refusal. Channel data may be a dataset of the `.h5` (an extension beyond WITSML 2.1, recognised by syntax); without a backend the answer is `DatasetNeedsFeature`, never an empty series.

## Tables

**A table's key columns come first, whatever order the document wrote them in** - row identity in a column-based table is positional and normative (common TRG section 9.1: "ordering matters since it will be the same in all columns"). Keys are unique nowhere, so lookup answers with the first matching row and `duplicate_keys()` reports the rest. A column that cannot be read does not silence the table: it is `None`, named in the notes, and the row count comes from the columns that read.

## Reading a whole package

`ObjectSource::object` returns an `Option`, enough for resolving a reference. `read_object` is provided (not required) and says *why* it failed (schema break versus object not held), because a whole-package walker needs the reason to name the right switch.

## Deliberately not here

- No rendering, no palette, no web or desktop code.
- No reprojection arithmetic (only the seam); no writing of version-specific CRS objects.
- No store dependency, no version-specific `match`.

## How it is tested

One test binary. Assertions state rules from the specification, with the document, section and page in a comment; values come from fabricated objects built in the tests. Cross-checks confront two independent paths (for instance the domain's expected property count against what the geometry reader actually reads; the colour tables against a second reading of the objects by field name). Checks that could pass on an empty set carry a count of what they covered. A table that is missing is a build failure, not a skipped test.
