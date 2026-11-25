# 0051 - The limits between two runs of values are classified, found and placed by three functions

Status: accepted.

## Context

Three readers want the same thing from a well: where one run of values ends and the next begins. The black line a section
draws between two stratigraphic units, the contacts a categorical property gives (the top of a facies), and the crossing
of a threshold by a continuous property (the top of the interval above 6 bar). Each was, or would have been, its own rule:
what a limit is, what to do when a class comes back further down, what to do next to a hole, where on the nodes a limit
sits.

The first decision that matters is the one the contacts operation took in its first version: **a class that comes back is a
refusal**, and the whole operation stopped at the first well that did. A pressure between 1 and 5 at the top, above 6 in the
middle and between 1 and 5 at the bottom is an ordinary well.

## Decision

`energyml_domain::limits` holds three steps, over the inputs of `frame_values` (and `well_strata` for units):

1. **Classification** (`Classification`): one `Option<Class>` per node or per interval of a frame. `None` is *no value* and no
   limit crosses it. The sources: the units of a stratigraphy (`of_units`), the codes of a categorical property (`of_codes`),
   one **column of its table** (`of_column`: two codes whose rows hold one value are one class), and a **threshold** on a
   continuous property (`of_threshold`): `value >= threshold` is `AtOrAbove`, `value < threshold` is `Below`. The equality is on
   the upper side and the words say so; they are the words the translations reuse. A threshold on codes, a code on a measure
   and a code that names no row are refused by name.
2. **Transitions** (`transitions`): the runs (maximal stretches of one class), the boundaries between two **adjacent** runs, and
   from each boundary two limits: the **base** of the run above and the **top** of the run below. **A class that comes back is a
   new run, never a refusal.** Each limit carries its **rank** (the *n*-th run of its class, so the first top, the second top)
   and the run it belongs to. A top exists only where something lies above it, a base only where something lies below: the top
   of the first run, the base of the last, and the sides of a run next to a hole have none.
3. **Placement** (`Transitions::place(limit, anchor)`), the measured depth of a limit:
   - on **intervals**, the node that separates the two intervals; the anchor means nothing;
   - on **nodes**, `Anchor::Inside` (a node of the run it bounds: the first node of a top, the last of a base, the default),
     `Anchor::Outside` (the neighbouring node outside the run), or `Anchor::Interpolated` for a threshold:
     `md = md_i + (s - v_i) / (v_j - v_i) * (md_j - md_i)`, linear in MD. A crossing that falls on a node (`v == s`) is that node.
     **Never over a hole**: two nodes further apart than `GapRule` allows (three times the median step) are refused, and an
     interpolation asked of classes that are not a threshold is refused.

The functions take no side: they decide nothing about a drawing, a contact or a file.

## Alternatives considered

- **Refusing a repeated crossing** (the contacts' first rule). It makes the ordinary well an error; the rank says which limit is
  which and lets the caller keep the first, the last or all.
- **A limit at the node by default for a measure.** It is one node of error on a threshold; the interpolated placement exists
  for it and stays a choice, because a limit written at a node is also what a log of coarse nodes should say.
- **A class that is a number.** Two classes are equal or they are not: an ordering of classes (which unit is older) is the
  business of the stratigraphic column and is not read here.

## Consequences

- The section drawing, the contacts and the black line of a strata change read the same limits; a rule is changed once.
- Nothing is connected yet: the sections of `energyml-section` (R4) and the contacts of `energyml-build` (R5) call these
  functions, and the contacts rule of refusing a repeated class changes there, as a public change, with its own entry.
- A threshold on a derived property (`FrameValues::derive`) is a threshold on a column of the table: the two steps compose.
