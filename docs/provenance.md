# Provenance

Where an object came from: which activity wrote it, which objects and files that
activity read, which step of which workflow it was. This page covers what the
library **writes** (so that a package records it completely), what it **reads**
(four releases of the schemas), and what it **shows** (the graph, the timeline,
and the provenance that some converters keep in tables instead of activities).

Every rule below follows the same line as the rest of the library: a departure
from what a template declares is **signalled, never refused**, and a statement
the file does not make is **not guessed** (see
[ADR 0005](adr/0005-refuse-rather-than-draw.md) and
[ADR 0035](adr/0035-provenance-read-and-written.md)).

## What the library writes

Every operation records an `Activity` against the canonical `ActivityTemplate`
of that operation (see [Creating objects](creating-objects.md)). Three points are
worth knowing when you read such a package:

- **`input` and `inputFile` are declared in every canonical template.** The EML
  technical reference guide 2.3 (section 3.1, p. 40) says a parameter's title
  *must have an equivalent in the activity descriptor parameters*. `input` (a
  data object, any number) and `inputFile` (a string, any number) are therefore
  declared by all of the canonical templates, so that a new package satisfies the
  rule.
- **A whole number is an `IntegerQuantityParameter`.** It has no unit in any
  release (section 3.8, p. 47). In Rust it is `Parameter::Integer`, which is a
  breaking addition to the `Parameter` enum.
- **A file a recipe step read is recorded.** A step that took a file (an
  `inputs = [{ file = ... }]` entry of a recipe) writes a `StringParameter` titled
  `inputFile`, whose value is `<slot>: <path>`, or just `<path>` when the recipe
  named no slot. The path is relative to the recipe, as the recipe wrote it.

## What the library reads

`energyml_domain::activity` reads an `Activity` and an `ActivityTemplate` of any
of the four releases (RESQML 2.0.1, EML 2.1, 2.2 and 2.3) into one shape,
`ActivityRecord` and `TemplateRecord`.

- A parameter's value is read **by the field present, never by the class name**:
  `DataObject`, `TimeIndex`, `Value` with `Uom` (a double), a text `Value`, or a
  whole number `Value` without a unit. A shape this build cannot read is shown as
  unreadable, never blank.
- A reference is kept as the file writes it (`DeclaredReference`): the full uri
  when it names a type, otherwise the bare uuid, the one exception the library
  makes to "full uri inside".
- A `DataObjectParameter` that points to an `Activity` is read as the
  `subActivity` kind, and a `TimeIndexParameter` as `timestamp`: neither kind has
  a class of its own in the schemas.

### Roles

For each parameter, `roles` answers *input, output, both, neither, sub-activity or
undeclared*, **and where the answer comes from**:

1. the template (the spec): its `IsInput` and `IsOutput` flags give four states
   (TRG 2.3 section 3.10, p. 49), and `subActivity` is the kind that names a
   child;
2. else a **named rule** of the registry (`RoleRules`; the caller's rules are
   tried first). The built-in rule, `energyml-build:input-output`, reads a
   parameter titled exactly `input` or `output` as such, only in an activity
   written by `energyml-rust`; it exists for packages written before the
   templates declared `input`;
3. else `Undeclared`. A role is never guessed from a title.

### Conformance

`conformance` compares an activity with its template and lists what departs:
a title with no equivalent, a kind outside the allowed ones, fewer parameters than
`MinOccurs`, more than `MaxOccurs` (a value of -1 means unbounded), or no template
at all. Nothing is refused: the activity is still read.

## The graph

`energyml_domain::provenance_graph` builds the graph a view draws, from activities
and, optionally, a package index.

- **Edges** say what an activity read (`Consumed`), wrote (`Produced`), merely
  took part in (`Participant`, when the role is neither or undeclared), its
  sub-activities and parent (`Parent`), the representation, interpretation or
  feature that carries an object (`Carrier`, from named rules, `CarrierRules`),
  and a collection's members (`Member`). Each edge keeps its **basis**: the
  parameter and the basis of its role, a reference field, a verbatim source, or
  both an activity and a verbatim source.
- **Nodes are keyed by the full uri**, a file by its path as written (`./a.las`
  and `a.las` are two nodes). Two activities that wrote the same object give one
  node with two `Produced` edges.
- **The tree** of activities is checked against both of its writings: the child's
  `Parent` and the parent's sub-activity parameters. They may disagree, and the
  disagreement is listed. A cycle or an absent parent is listed and the activities
  are shown flat; nothing loops.
- **Held steps.** A workflow template declares its steps. A step declared but not
  run (it has `MinOccurs` 0 and a reason in its constraint) is a node of its own,
  with its reason; one declared held that ran anyway is signalled.
- **Set aside.** Objects of some types (a CRS, a property kind, an activity
  template) are not nodes: they are listed beside the graph with what named them.
- **Absent outputs.** An object an activity says it wrote and the package does not
  hold is signalled.
- **`focus`** keeps a node and what lies upstream and downstream of it, by a
  number of edges (at most 64).
- **`timeline`** orders activities by the instants the file states. When there are
  fewer than two distinct instants it refuses with `NoMeasuredOrder` and says which
  order it shows instead; it does not invent a time.
- **Collections.** `PackageIndex::expand_collection` lists the members of a
  `DataobjectCollection` from the association sets that name it. A collection
  that no set names is refused: it states no members, which is not the same as
  having none.

## Provenance kept in tables (verbatim sources)

Some converters record provenance in tables rather than in `Activity` objects.
`energyml_domain::provenance_verbatim` reads such a convention as written and
compares it with the activities.

A `VerbatimSource` recognises a set of tables **from the titles of their columns
only** (no array is read to recognise), and reads them into `VerbatimProcess`es.
The sources are a registry (`VerbatimSources`), the caller's first. The built-in
source is `NwdfTables`, for the NWDF v0.2 tables of the UDAR converter:

| Role | Columns |
|---|---|
| processes | `process_uid`, `activity` |
| links | `source_uid`, `attribute`, `target`, `target_table` |
| mapping (NWDF uid to EPC object) | `nwdf_uid`, `role`, `epc_uri` |
| references (optional) | `path`, `reference_uid` |

How they are read:

- **One table per role.** Two tables playing the same role are refused with
  `SeveralTables`, naming them: the library does not guess which group a table
  belongs to. A missing or doubled column is named too.
- **A process's inputs and outputs** come from the links whose `source_uid` is a
  `process_uid`, never from the `source_table` title. A target `external file` is a
  file (with the key a references table gives its path); any other target is
  resolved **through the mapping table**.
- **A key the mapping does not tie is `Unresolved`**, shown as such. It is never
  resolved by its title: a title only looks like the object.
- **The activity of a process** is stated twice, by the `activity` column and by the
  mapping's `eml activity` row. If they name different activities,
  `ActivityLinkDiffers` is signalled; the column wins. A process naming no activity
  draws no edge and is signalled.
- **Every other column** of the processes table is shown verbatim. An empty text is
  a value; a missing number is an absent cell.
- The `dataset_uids` links are acquisition context, not processing provenance, and
  are left to another view.

`reconcile` then compares what an activity says (the objects its parameters read
and wrote, by role) with what the tables say, and sorts each object into *both*,
*only the activity*, *only the tables* (files included) or *roles differ* (stated
in opposite directions). Two objects are the same by uuid and agreeing versions,
never by text.

## From a workflow to a recipe

`draft_from_workflow` in `energyml-build` rebuilds a recipe draft from a workflow
that was played: the steps, their settings and the files they read (`inputFile`).
The draft is **lossy** and says what it left out. It refuses what it cannot restate
(for example a step whose operation it does not know) with a named
`ReplayRefusal`, rather than writing a recipe that would not reproduce the package.
The constraint of a step (`follows <template>`) is read by `read_step_constraint`,
which lives beside the code that writes it.
