# 0014 - Horizons and units, attachments, business constraints

Status: accepted

Three small decisions about links the standard leaves implicit.

## Horizons and stratigraphic units

### Context

RESQML expresses the relation between a horizon and the stratigraphic units it
separates in exactly one place: a contact. If nothing writes that link, a viewer
has nothing to read for the contact between units, and a fallback of "colour a
surface with the colour of its unit" has nothing to follow.

### Decision

The structural link between a horizon and its units is written through the
contact the standard provides, and readers follow it. The final fallback of the
colour cascade (ADR 0009) may use the unit's colour; which side keeps which colour
when two units differ is a library decision, recorded as a house rule.

## Attachments

### Context

Files (images, documents) can sit inside the zip or in an `Attachment` object.

### Decision

A file is tied to an object by a **relationship**, never by its content type.
The content type says what a part is; the relationship says which object it
belongs to. Attachments are a **local-package** capability (a separate trait from
the dataspace trait, ADR 0002) because a dataspace transports objects and arrays,
not files. The declared type of a file is never guessed, and a file attached to
nothing is written and **reported**.

## Business constraints

### Context

The schema cannot type the target of a reference, and the legitimate associations
are written only in the technical guides.

### Decision

Business rules (what a reference may point at, what two linked objects must say
together) are a layer above schema validation, reference integrity and count
coherence, and are executable. They are separate checks with named findings, not
mixed into the schema check.

## Consequences

- Links the standard implies are written and followed, not inferred.
- Each check reports which layer found the problem.
