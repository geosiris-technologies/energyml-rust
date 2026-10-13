# energyml-sheet — architecture

Related decisions: [ADR 0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR 0011](../../docs/adr/0011-tests-follow-the-specification.md), [ADR 0016](../../docs/adr/0016-one-crate-one-test-binary.md).

## Purpose and position

Computes a two-dimensional well sheet (a borehole log drawn as a strip chart: depth down the page, one column per track) and writes it as SVG. It sits above `energyml-storage`, `energyml-domain` and `energyml-geometry`, and below every face. It compiles to WebAssembly with no feature flags.

Three stages, each testable alone: `read` (a frame of a package becomes a `Board`), `layout` (a `Board` becomes positioned cells; pure), `svg` (the single output point; it holds no logic that is not already in the layout).

## Rules

- **`layout` knows no package.** `Board` is plain data: no object source, no array source. The layout is then tested with numbers, without an EPC or HDF5, and can be reused over any storage.
- **Only interval tracks cut the sheet.** The depth bounds are the union of the `[top, base]` of every interval track, never those of a per-node track. Nothing in `layout` reads a whole frame, so a frame with tens of thousands of stations cannot create a section per station.
- **An interval carries text and colour, and a track may render both.** `IntervalTrack::renders` is a list (`fill`, `text`), not a binary choice, so a coloured section can carry its stratigraphic label in the same column.
- **The readable mode is a level-filling algorithm, with an invariant.** Remaining budget is never smaller than the sum of the active minimums; this survives every round whatever subset is frozen, so heights always sum to the requested height. Breaking the layout test means breaking that argument, not a numeric coincidence.
- **No default for what is not decided.** `max_chars` / `min_chars` have defaults; `Params` (scale, height, page width, text size) has none. A default scale lives in the face that picks one.
- **The page grows, the columns do not shrink.** Squeezing eight 32-character columns into A4 made descriptions overlap, so the page width follows the columns.
- **`font-size` is written in the unit of the `viewBox`** (millimetres); `pt` there is several times too large for the room the layout reserved.
- **Two columns never share a header.** A shared property kind yields to the title of each property, minus the whole words it shares with the frame title; this is reported in a note.
- **An all-null column is still drawn**, blank and counted: that is what the file says. Hiding it is a track-selection decision, not a reading one.
- **Text widths come from a real font file, never from a chosen number.** The width table is generated from Liberation Sans (SIL OFL 1.1, checked in the font file itself); the font is never copied into the repository, only the generated table. Do not edit the table by hand. There is no kerning, so a kernable pair measures slightly wide, within the tolerance `max_chars` already accepts.
- **Sort depths with `f64::total_cmp`.** A depth should never be `NaN`, but a sort that panics on a malformed file is the wrong place to find out.
- **Dropped text is counted.** The connected overflow (a trapezoid joining a displaced box to its section) is not drawn; only truncation with an ellipsis is implemented, and each truncation is counted in the notes.

## Deliberately not done

Curves, lithology patterns, PNG/PDF output and a per-column width decision (column widths are a documented heuristic of the `svg` module). Each is absent rather than approximated.

## Tests

`tests/layout.rs` and `tests/svg.rs` assert numbers: height proportional to thickness at scale, the readable mode summing to the requested height with order kept, monotone positions, a refusal named when `max_chars < min_chars`, no line wider than its measured width, an empty interval reported as a stated blank. The SVG is re-read by an XML parser that is not this crate's code. Run them in the workspace, not with `cargo test -p`: `cargo test --workspace --test layout`.
