# 0027 - Gaps, seams, scales and quantiles: the numbers the views share

Status: accepted

## Context

Several views read values along a measured depth (a log column, the 3D paint, a
section, an object's statistics). Each needs the same few answers: may two
neighbouring nodes be joined, is a sudden step of the value a splice, on which
scale does a quantity read, which bounds does a colour map take when the file
states none. The standards say how a value hangs on a node or an interval and
how a colour map maps its values; they say nothing about these four questions.
Left to each view, each would answer a little differently, and a number on
screen would depend on the view that drew it.

## Decision

- **A gap is never bridged beyond 3 median steps (S7).** The limit is
  `GAP_RATIO = 3` times the median step of the axis (`along_md::GapRule`). A
  caller may be stricter (`stricter`), never looser: the limit is private. A gap
  is drawn empty, and an interpolation across it refuses with
  `GapTooWide`. A node is not a bridge: on a node (`t` of 0 or 1) the value is
  the node's, whatever the gap.
- **A seam is signalled beyond 10 median steps (S7).** A step of the value
  larger than `JUMP_RATIO = 10` times the median step of its variation is a
  `Jump`, reported with its place and its ratio. The values are kept as
  written; the drawing is not smoothed across it. The two thresholds sit in the
  same module so that they are read together.
- **The logarithmic scale is proposed from 3 decades (S8).** When the finite
  positive values span at least `LOG_DECADES = 3` decades the log scale is
  proposed. It is a proposal: a scale the file states or the user chooses wins,
  and a disagreement with the file is said. Angles taken as linear are accepted
  with the note `AngleTakenAsLinear`, not refused (detect and signal, do not
  forbid).
- **Automatic bounds are the 1st and 99th percentiles (S8).** Over the finite
  values (the positive ones on a log scale). The minimum and the maximum stay
  available.
- **Quantiles are exact up to 10 million values, then a sketch.** The exact
  definition is the linear one of Hyndman and Fan, type 7 (the one most tools
  call the default percentile). Beyond `EXACT_QUANTILE_LIMIT = 10 000 000` values
  the answer comes from a mergeable DDSketch (Masson, Rim and Lee, 2019) of
  relative error 1 % (`SKETCH_RELATIVE_ERROR`), and the result says that it is
  a sketch and what its error is. TODO(verify): the number of the theorem of
  the paper that states the guarantee.
- **On a tie, the nearest node is the shallower** (the smaller MD, then the
  smaller index): a written rule, stable between views.
- **Opposite angles are refused.** Interpolating between two angles takes the
  shorter way round; when the two are opposite (within 1e-12 rad) neither way is
  shorter, and `angular` refuses with `OppositeAngles` rather than choose a
  direction.

The four rules of the registry (`house/stats/*`) state these numbers; they are
declared rules (no check on a package), and a test crosses their text with the
constants of the code (`tests/stated_numbers.rs`).

## Consequences

- One arithmetic for every view: a number that differs between two views is a
  defect, not a taste.
- A caller that needs a stricter gap says so; none can draw across a hole the
  rule refuses.
- The numbers are conventions of ours, not of the standards. They are in the
  registry with their reason, so that anyone can disagree with them explicitly
  (ADR 0005).
- Changing a number is changing a constant and its rule text together; the cross
  test goes red until both agree.
