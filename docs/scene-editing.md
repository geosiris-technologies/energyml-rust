# Editing a scene

A viewer lets a person move a surface, cut it with a polygon, delete some of its
nodes, or draw a new surface between two lines. In this library each of those
gestures is an **operation** of `energyml-build`, the same kind of object that
creates wells or interpolates a surface. There is no separate editing engine, so
a gesture a page offers can be written in a recipe, run from the command line or
called from Python with the same result.

## The rule they share

Like every operation, an editing operation is a pure function from a bag of
pieces to a **plan** ([ADR 0017](adr/0017-operation-is-a-pure-function.md)):

1. **Plan.** The operation reads the shape it is given and returns what it would
   write, the notes that say what it decided and what it counted, and the
   activity record. Nothing is stored and nothing is modified.
2. **Apply.** Only `apply` touches storage, and it only **inserts**: an editing
   operation produces a new object (a new representation of the same kind), and
   the object it started from stays untouched in the package. Whether a page
   shows the result as a copy or in place of the original is the page's choice.

The one exception is `edit-object`, which is explicit about it: it rewrites an
object under its own identifier, or copies it under a new one, and says each
change (old value and new) in the plan.

Two more rules follow. A gesture that would silently do nothing, or silently do
something else, is **refused by name** (an axis of length zero, a node that does
not exist, a cut that removes everything or nothing). And the activity written
with the result records which operation ran and with which settings.

## The operations

| Operation | What it does |
|---|---|
| `transform` | moves, turns and scales a shape, or some of its nodes, about one pivot: the scale first, then the rotation, then the translation. The topology is copied as it is |
| `change-frame` | reads a representation in one frame of the package and writes it in another, the same nodes in the same order; depth to time takes a stated, sourced velocity law and marks the result synthetic ([ADR 0019](adr/0019-synthetic-data-is-flagged-and-sourced.md)) |
| `clip` | cuts a shape by an outer ring and inner rings; the cut follows the nodes, not the ring |
| `delete-nodes` | takes some nodes out: a cloud loses them, a line is cut at them, a surface loses the triangles that name them |
| `densify` | adds points between the vertices of a polyline and keeps every vertex |
| `loft` | builds a triangulated surface between polylines taken in a declared order |
| `project` | projects nodes vertically onto a surface or a level; a node with nothing above or below it is left where it was and counted |
| `remesh` | redoes the topology of a triangulated horizon and keeps its nodes |
| `resurface` | recomputes a shape's own heights, keeping its topology; the rings only select which points inform the fit |
| `edit-object` | sets fields of an object as told, by path in the JSON encoding of the object |

The computations live in `energyml-analysis`; the operation is the seam that
reads pieces, calls them and writes through the writer of the shape's own kind.

## What is refused

- A grid's cells or a volume where the operation has no writer for it: writing a
  moved grid would mean choosing between a triangulated surface and a lattice
  with turned vectors, and that is a decision, not a default.
- A shape holding patches of several kinds: one representation holds one kind.
- Two shapes that name two different frames, when one of them is a grid: a grid is
  never moved to another frame to make them agree
  ([ADR 0020](adr/0020-a-grid-is-never-reprojected.md)).

To see what an operation would do without writing, ask for its plan (see
[Creating objects](creating-objects.md)).
