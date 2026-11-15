# 0031 - The direction of a line at a knot is that of the interval that ends there

Status: accepted

## Context

Every view around a well asks the same question: in which direction does the hole
go at this measured depth? The line of a trajectory is evaluated by
`Path::at`, which for a linear line interpolates between two knots. At a knot itself
a polyline has two directions, one on each side, and the specification of a
parametric line states none. A frame, a ring or a projection drawn at a knot
would be different depending on who computed the direction, and a view that
mixes a point from one segment with a direction from the other draws a frame
that does not sit on the line.

Two ways to get a direction were on the table: a centred difference of `at`
(cheap, written everywhere), or the derivative of the very interpolant `at`
evaluates.

## Decision

- **The direction is the derivative of the interpolant that `Path::at`
  evaluates**, kind by kind (linear, natural cubic spline, Hermite, minimum
  curvature, vertical), in `Path::direction_at`. It is not a second curve and
  not a difference of two evaluations.
- **At a knot of a linear line, the direction is that of the interval that ends
  there**: the interval in which `at` evaluates that parameter (`Line::segment`).
  At the first knot, where no interval ends, it is the direction of the first
  interval. Nothing more is coded: the rule is the one `segment` already
  applies, which keeps a point and its direction on the same segment.
- **Past either end**, the derivative of the extended interval, as `at` extends it.
- **Where the derivative vanishes, is not finite, or the line does not exist**, the
  answer is `None`, never a zero vector.
- The rule is registered as `house/well/tangent-at-a-knot` and is read by the
  frame of a hole, by `PlacedPath::direction_at` and by every projection.

## Consequences

- A test can compare the analytic direction with a centred difference of `at` away
  from the knots: two independent sources of the same fact. At a knot the two
  disagree on purpose, and the test says which interval wins.
- A view that wants the direction of the interval that starts at a knot asks for a
  parameter a hair after it; there is no second function for it.
- A change of how `at` finds its interval changes the direction with it, which is
  the point: they cannot drift apart.
