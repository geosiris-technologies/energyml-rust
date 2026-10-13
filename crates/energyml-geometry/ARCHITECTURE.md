# energyml-geometry: architecture

## Purpose and position

Turns a representation into points and topology, for every representation type of the RESQML family: meshes and surfaces, IJK grids (faults, K gaps, parametric pillars, local grids), unstructured grids, grid connection sets, wellbore trajectories and frames, planes, sub-representations, and geometry borrowed from another representation. It sits above `energyml-storage` and `energyml-domain`, never inside them, and below `energyml-export`, `energyml-cache` and every face.

**Refuse rather than draw.** An empty geometry looks exactly like a program that works and has nothing to show, and a *wrong* mesh looks exactly like a right one. Where the specification is ambiguous the answer is a named refusal, not a guess; when a guess is finally settled, it is settled by a proof written beside the code.

Related decisions: [ADR-0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR-0006](../../docs/adr/0006-registries-rather-than-match.md), [ADR-0011](../../docs/adr/0011-tests-follow-the-specification.md).

## Readers and versions

**Registries, not matches** — representation readers (like array backends and exporters) are a trait, a registry, built-ins pushed by default, and caller additions tried first so a built-in can be replaced.

**Readers are keyed on `type_key`, so a new version costs nothing** — what costs something is a release that respells the inside of a type. Those spellings live in named constants beside the reader (`NODE_PATCH`, `SPLIT_FIELDS`, `PARENT_GRID_FIELDS`, `INTERPRETS`, `PATCH_OF_VALUES`), each the list of names one concept has worn; a new spelling is one string in one list, never a `match` on version.

**What a document states is not what a reader places** — `declared_nodes` is the cheap half of reading: how many nodes the document claims, without opening a dataset (listing the drawable objects of a package otherwise costs one evaluation per representation). `None` is the ordinary answer and is not zero (a polyline set keeps its counts in an array; a sub-representation states elements, not nodes). Declared and drawn are two quantities: a lattice states its whole grid and the reader places only nodes with a location. The caller shows the declared number *as declared* and never overwrites it with the measured one. Read the field that is present, never the version. A cross-check holds every reader to this: no reader places more nodes than its document states.

## Points and lines

**An unevaluated point array fails by name** — `AbstractPoint3dArray` has five forms and only one reads a file; returning no points for a form not evaluated is indistinguishable from nothing to draw. A successful evaluation that yields nothing is a bug: a patch with no geometry is an error naming the patch (the 2.0.1 `NodePatch/Geometry` and the 2.2 `NodePatchGeometry` spellings hid points behind a passing test until the check demanded a positive count).

**Patch node numbering is decided from the numbers** — triangle indices may be local to the patch (resqpy) or global to the representation (fesapi); the schema does not settle it and both files exist. The reader takes whichever range contains every index and refuses one that fits neither. The two agree on patch 0, which is why choosing a side passes every single-patch test.

**A lattice's dimensions run slowest first** — the last one varies fastest. Reversing yields the right count of finite points and a transposed surface that no downstream assertion catches; a check compares the run length to the declared fastest-axis count.

**A parametric line kind** — `-1` null, `0` vertical, `1` linear, `2` natural cubic, `3` tangential cubic, `4` Z-linear cubic, `5` minimum curvature. An index outside the table is refused by name rather than approximated.

**A parametric line is unbounded** — a parameter beyond the knots extends the first or last interval, never truncates (2.0.1 `Geometry.xsd`: otherwise "the interpolant in the first or last interval is used as an extrapolating function"; RESQML TRG 2.2, section 3.8: "In general, a parametric line is unbounded"). The `isBounded` flag exists in neither schema, so no file can ask for truncation. Truncating collapses every faulted column onto its pillar end.

**A vertical parametric line stores a depth, and the CRS says what depth becomes** (RESQML TRG, section 3.8) — a depth-increasing-downward system keeps it, an elevation system negates it. The reader takes the vertical convention from the geometry's own CRS and requires it as an argument, because a default is right for depth systems and mirrors every well and pillar about its datum in the others. The conversion belongs to the reader, not to the placement layer, which would flip it twice. A geometry naming no CRS keeps the file's numbers.

**A spline is evaluated, never straightened** — minimum curvature is the arc itself (the direction turns at a constant rate; the position is its integral), reproducing the drilling formula on arc-consistent data. Where no arc passes through both knots, a linear term closes the gap so the curve still reaches its knot: that term is this crate's decision, not a quotation.

**`KnotCount` is the longest line in the array and short lines are padded with NaN** — a line's real length is its leading run of finite control points; reading padding as data poisons every point on the line, silently, because NaN propagates.

**A datum written in a compound is placed once** (common TRG 2.3, section 13.25, p. 371): a well without geometry is anchored by its datum, whose coordinates are local to the compound it names and go through that compound's projection; the CRS identity returned is the compound's without its placement, so the two read together do not place the well twice. A time-based compound is refused: an MD is a length.

**A WITSML trajectory is drawn from its stations, never from `survey`** — measured depth, inclination and azimuth (WITSML TRG 2.1, section 16.10) placed by minimum curvature. It lives here, not in the analysis crate, because reading a trajectory is reading a shape and analysis sits above geometry. The datum is the one the trajectory names, placed by the single datum reader for RESQML and WITSML alike. North is the stated azimuth reference: grid as is, plane by the compound's azimuth; true, magnetic and the others need a convergence or declination and are refused, as is absence unless the well never leaves the vertical. A first station away from the datum (a tie-on) is refused rather than attached to the datum. No stated algorithm other than minimum curvature is accepted.

**A tube has a ring per station** — it sweeps the polyline as it is (the points are already the evaluated curve; smoothing would draw a path the file does not state), and a ring per station makes colouring a one-liner by station index. A radius, never a diameter: converting silently on one side of an interface is how a well comes out twice too big.

## IJK grids

**An IJK corner is a lookup, not an index** — split coordinate lines are extra pillars appended after the regular ones, and a column across a fault uses them. Ignoring them does not fail; it draws cells pulled across every fault.

**K gaps add a level of points** — `Points` holds `Nk + 1 + gaps` levels, and layer `k` no longer sits between levels `k` and `k+1`; reading it as ungapped gives cells spanning the void. The top of layer `k0` is a level (`ijk::layer_tops`), not `k0`.

**Cell winding is measured, not deduced** — right-handedness, K direction and the CRS's z flip interact (eight combinations, one answer). The face normal is compared against the vector to the opposite face and swapped when they disagree, because a hexahedron built the other way round renders as a hole.

**A local grid has no geometry of its own, and that is not a gap in the file** — the parent window and a regrid per axis give the child's points as the parent's, subdivided. Each axis is reduced to where child node *n* sits in parent cells from the window start, then the parent's pillar lattice is interpolated there. Interpolating in pillar space rather than cell space keeps the child watertight. A fault or K gap *inside* the window is refused (the boundary node would have two positions); outside, nothing changes. Only the window is read, by hyperslab, one per row of pillars; a test counts the values handed out, since a silent fallback to "read everything" passes every coordinate assertion.

**A node that no cell names is not a location** — a cell flagged undefined by `CellGeometryIsDefined` is skipped (the flag overrides the numbers, which may stay finite), and nodes of hexahedral or polyhedral patches that no cell names become holes and are counted. Surfaces, lines and point sets keep lone nodes: there a node is a sample in its own right.

## Unstructured grids

**A cell is enumerated, not computed** — two hops of jagged arrays (faces per cell, nodes per face); neither is constant-width, and a grid mixing a tetrahedron and a prism, triangles and quadrilaterals, sharing a face, proves it.

**`CellFaceIsRightHanded` is indexed per cell-face pair, not per face** — a shared face is stored once and wound one way, so it points out of one cell and into the other. The schema says the array's length is the sum of the cell face counts, exceeding the face count by one per shared face; the length is checked and the check quotes the rule. The right-hand rule is stated in the projected frame (a depth-down system makes stored coordinates left-handed), verified by measuring normals against cell centroids after projection.

**`CellShape` is a hint and is not trusted** — "supplied by the RESQML writer ... as an aide to the RESQML reader" with a "should" business rule; counts come from the cumulative lengths.

## Planes, connections, sub-representations

**A tilted plane is drawn, a horizontal plane is refused, and the boundary is what the file states** — a tilted plane carries three 3D points, which are locations, so the triangle joining them is those three points. A horizontal plane carries one coordinate: a plane at a depth is infinite, and its extent is a decision about the picture, belonging to whoever draws. Hence `Error::Unbounded`, distinct from `Unsupported`: unsupported means this build does not evaluate the form yet (waiting is a strategy); unbounded means the file will never say. Several triples under one tilted plane need no ruling to be drawn (each triangle is three stated points); anything that computes on the plane would.

**A grid connection set has no geometry, by design** (RESQML TUG, section 9.16: "Unlike a grid, geometry as such is never attached to a connection")**; its image is drawn from the grids it targets.** Where local face indices are present, the named faces are drawn; where absent (a non-neighbouring connection has no shared face), a segment between the two cell centres, which is all the file states. The local face order is specified for both grid kinds (TUG section 9.12 for IJK; section 9.13 for unstructured). **A cell is addressed by the file's cell number, never by position**: the hexahedra topology compacts undefined cells, so position stops being the RESQML number as soon as a grid has a hole, and connection sets exist precisely for faulted grids. `Topology::position_of` translates and returns `None` for a discarded cell. Both halves of a connection are assembled before anything is written; anything stated per connection is aligned by `read_with_interpretations`, which returns the patch and the original connection of each entry in one walk. Nothing here says "fault": a connection may represent a horizon or geobody boundary; the interpretation says what it is.

**A sub-representation cannot slice like a local grid** — its selection is an arbitrary index list, not a box. A bounding-range-plus-gather fast path is deliberately not written until a real use exists. Also, "sub-representation" is two objects wearing one name: in 2.0.1 one supporting representation sits on the object and one or two element-index sets on each patch; in 2.2 the indexable element moved up and the supporting representation down to each patch. Neither is translated into the other, and two element-index sets on one patch (a list of pairs, with no statement of which carries the geometry) are refused.

## Whole packages and transfer

**One door: `scene::of_package`** — it reads a package into scene objects with properties and colours, and it used to be written three times (two commands and a language binding) which had begun to diverge. It does the walk over the inventory (type filter before deserialisation), builds scene objects, attaches properties and resolves their colour ramps. It does not decide placement, reprojection, tube sweeping or which exporter: those are what to do with shapes. The source is a value source and a list of summaries, never a store. It has three refusal lists, not one (unreadable, not evaluable, not attachable: three problems fixed in three places). Properties default to none, because attaching reads every property of the package and their external arrays; they are read once for the package and grouped once (`PropertiesOf`), never per shape. A colour source is injected through a trait (`scene::Decide`), so this crate never depends on the cache crate; without one it falls back to the short, index-free colour order. `SceneObject.qualified_type` carries a real qualified type (including the release), the one line of an export that leads back to the package.

**Averaging a category is wrong** — `transfer` moves values between points and cells with a named method, never a guessed one: majority for a property with a category lookup, average otherwise, and `NaN` is discarded rather than propagated. The frame intervals go onto the nodes (the interval a node opens, the deepest node the one it closes), only for a frame that is a single polyline, and the note says so.

**A closure is read, not guessed** — `Patch::closed` carries per line what `ClosedPolylines` states and is empty when the file says nothing (not "all open"). A closing segment returns to the first node without repeating it (a ring counts its nodes once; repeating would shift any property indexed on them), and lines of fewer than three nodes are not closed.

## Deliberately not here

- No storage code, no file format, no rendering and no analysis (interpolation lives above this crate).
- No guess when the specification is silent: a named refusal instead.
- No dependency on the cache crate.

## How it is tested

One test binary. The shape of each assertion comes from the specification (a stated count is *the* number of nodes; a face winding is outward after projection) and values come from fabricated fixtures built in the tests. Several tests were broken on purpose to prove they can fail (a gapped parent grid broken both ways; counting values read to catch a silent full read). Tests against local real packages are feature-gated and off by default.
