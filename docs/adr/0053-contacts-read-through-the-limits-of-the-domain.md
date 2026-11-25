# 0053 - `contacts-from-frames` reads its runs through the limits of the domain, and a class that comes back is no longer refused

Status: accepted. Changes a public rule of `contacts-from-frames`; builds on [ADR-0051](0051-the-limits-between-two-runs-of-values.md).

## Context

`contacts-from-frames` found the top and the base of each unit a frame crosses by walking the runs of the frame's units
itself, and **refused the whole operation** at the first hole in which a unit came back (A, B, A): "which one is the
unit's top is not for this operation to decide". One such well stopped a run over a thousand. The rule of a run (what
ends it, what a hole between two intervals does, what is a top and what a base) was written there, and again, differently,
in each place that cuts a well in classes.

Two readings were asked for besides the units: a **categorical property** (the facies, the zone) and a **threshold** on a
continuous one (the top of what is above 6 bar) - the same question, on another suite along the well.

## Decision

- **The runs are the domain's.** The operation builds a `Classification` of each frame (its units, the codes of a
  property, or the two sides of a threshold), takes the `transitions` of `energyml_domain::limits` and writes a contact
  for each limit of a class it keeps. It keeps no rule of a run. `Reading` (the rank of the chart) still says what the units
  of the intervals are; nothing else of it changed.
- **A class that comes back gives several contacts, never a refusal.** The point set stays **one per (class, side)**, its
  uuid derived as it always was, and each point carries its **rank** - 1 for the first top of the class in that hole, 2 for
  the second - in a `DiscreteProperty` `<point set> - rank` (kind `explicit index`, the dictionary's). It is written on every
  point set when any rank is above 1, and on none otherwise: **a run with no repeated crossing writes exactly what it wrote.**
  The person chooses with `which`: `all` (the default), the `first` or the `last` contact of each class and side in each hole.
- **Two readings more.** `categoryKind` (with `codes`) classes a frame by the codes of its discrete property of that kind,
  on nodes or intervals; `thresholdKind` and `threshold` by `value >= threshold` against `value < threshold`, the contacts
  written being those of `thresholdSide`. One reading at a time; `rank` and `unit` read the units and do not mix with them.
  The classes' point sets are titled after the kind (`facies 2 - top`, `porosity at or above 0.2 - top`) and keyed by it, so
  their uuids do not touch those of the units. Titles aside: a class is **titled with its label** when its table gives one
  (`sand - top`; `sand (2) - top` when two rows bear one label), and **chosen by its number or its label**, `codes` and
  `labels` being a union (a class is kept when either names it), a label no row bears being refused by name. The rows of the
  table reach the plan through `Known::labels`, read at the boundary by `frame_values::category_labels`, the domain's one
  reading of a category table.
- **Where a contact sits on nodes** is `place`: `inside` the run it bounds (the default), `outside` it, or `interpolated`
  where the threshold is crossed, linear in measured depth between two nodes, never over a gap `GapRule` refuses. On intervals
  it is the node between two of them.
- **A well the reading cannot treat is refused alone**, named with its reason in the plan (a frame with no property of the
  kind, with two, one that does not read, a label its table does not bear, no trajectory to place an interpolated point on, a
  node count that is not its intervals' plus one), and the operation goes on with the others. It was an error of the whole run.
- **An interpolated point lies on the trajectory**, at the measured depth: the plan evaluates the line the frame names
  (`Known::path`, read at the boundary by `energyml_geometry::well::along_hole`, the one door `well::frame` places its nodes
  with). On a deviated hole the chord between two nodes is beside the well, and a point there looks exactly
  like a right one, so it is never the fallback: a frame whose trajectory is not in the package, does not read, states no
  measured depth at its stations, or is drawn in another frame than its line (the check `computed-tops` makes) is **refused
  alone** and named. On `inside` and `outside` the point stays the node. The properties copied onto the point are read at its
  MD (`FrameValues::at_md`): a property on intervals takes the interval that holds it, a continuous one on nodes is
  interpolated, a code is null (held at its node).
- **One door for the point of a trajectory at a measured depth**: `energyml_geometry::well::along_hole` gives an
  `AlongHole`, which is the trajectory's own line when it has a `Geometry`, and when it has none the vertical read from its
  datum, flagged (`is_assumed`) as the assumption it is. `well::frame` places its nodes through it, and so does the interpolated
  reading here (`KnownPath::along`): a trajectory the file gives no geometry is no longer refused, and its contact lies where
  the well is drawn, never beside it. The plan says so (`contacts-on-datum-vertical`), the assumption not being the file's.
  `well::path_of` keeps refusing the case, for what writes a depth as data and must not carry the assumption without saying so:
  `computed-tops`, the only other user of `path_of` (through `Known::path`): it writes the depths where the line meets a
  horizon, a fact, and shares the placement check (`drawn_elsewhere`) with the interpolated reading through `AlongHole`.

## Alternatives considered

- **One point set per rank.** It changes the uuids of the clouds and the number of objects of a run with no repetition;
  the rank in a property keeps both.
- **A rank property always.** Every existing output would gain objects. Written when it says something.
- **Choosing the top for the person** (the first, say). That is the decision the operation used to refuse to take.

## Consequences

- A hole A, B, A no longer stops the run: it gives two tops and two bases, each point ranked. The CHANGELOG says it: **a
  change of public rule**, and the two tests that expected the refusal now expect the contacts.
- A frame that cannot be read is in the plan's `dropped` and its `says` (`frames-refused`), never an error of the run, unless
  no frame gave a contact.
- The contacts of a facies or of a threshold are the same objects a unit's are, under another title.
