# 0041 - A convention the file does not state is declared by the caller; a vertical without a CRS is signalled

Status: accepted

## Context

Several readings around a well depend on a fact the files do not say: where an
angle around the hole starts and which way it grows, which dimension of a
per-node matrix is the slow one and the sign of its lateral axis, which hand a
placed frame has. Two readings of the same array give two pictures, and both
look plausible; only the producer knows which is right. A library that picks
one draws something that looks like data.

Another case is a line read without any coordinate system. The conversion from
depth to `z` needs to know which way is up. Without a CRS the library has long
taken depth as downward, which is the usual case and also a guess.

## Decision

- **A convention the file does not state is a parameter**, declared by the
  caller (`AngleConvention`: origin, sense, side from which it is seen, offset;
  `TransverseGrid`: the axis, the sign and the offsets of each dimension; the
  handedness of a frame in `FrameOptions`). Until it is declared, the layer that
  needs it is refused by name (`ConventionUndeclared`). There is no default
  and no "unknown" state: the caller chooses `Regular` or states a convention.
  It is registered as `house/well/conventions-are-declared`.
- **A declaration is never written into the package** by these functions, and it
  never overrides what the file states (ADR 0032). Writing one is the business
  of an operation that edits a working copy, not of a reading.
- **The canonical plane is fixed**: as seen by an observer standing uphole and
  looking down the hole, `right` to the right and `high_side` up, a clockwise
  turn going from the top to the right. A convention is written against it.
- **A vertical without a CRS is kept as depth downward and said so.**
  `Vertical::of` is unchanged; `Vertical::reading` and `read_noted` return a
  `VerticalReading` whose `VerticalBasis` is `Crs`, `NoCrs`, `UnreadableCrs` or
  `Declared`. The basis travels with the line (`Path::vertical_basis`,
  `Path::vertical_note`), and `PlacedPath::new` sets it with
  `Path::with_vertical` when the caller declares the system. A view says
  that the vertical was assumed instead of drawing it as if it were stated.
- Placing a path without a CRS is refused (`PathRefusal::NoCrs`); declaring one
  goes through `PlacedPath::new`.

## Consequences

- A caller with the producer's documentation writes its convention once and gets
  the same picture every time; one without it gets a named refusal and not a
  plausible wrong ring.
- A convention such as the order of the axes of a vendor's compound system is
  settled in a campaign on real files, never guessed by the library.
- A test that declares a convention states which reading it stands for.
- The signal on the vertical does not change a number: it adds a note that a view
  can show.
