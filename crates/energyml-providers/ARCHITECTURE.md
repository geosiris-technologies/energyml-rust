# energyml-providers - architecture

Related decisions: [ADR 0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR 0006](../../docs/adr/0006-registries-rather-than-match.md), [ADR 0011](../../docs/adr/0011-tests-follow-the-specification.md), [ADR 0012](../../docs/adr/0012-no-face-carries-logic.md).

## Purpose and position

Querying public geoscience web APIs (OGC WFS services, an elevation raster service) and turning the answers into material for `energyml-build`. Its only workspace dependency is `energyml-build`, for the GeoJSON reader that parses the answers. No storage, no geometry: what the entities *mean* is the business of the build operations.

## The boundary

> **Building a request and reading a response are pure. The round trip is not.**

Everything that can be wrong is pure and tested without a network: encoding a filter, the axis order of a bounding box, reading a server refusal, reading a quote. The round trip is the `Transport` trait, with a native implementation behind the `native` feature; in a browser the host's `fetch` plays that role and nothing here is used there.

## Rules

- **Axis order depends on the parameter, and it is a field, not a rule.** The same service wants `lat, lon` in a `bbox=…,urn:ogc:def:crs:EPSG::4326` parameter and `lon, lat` in a `cql_filter=Intersects(…,SRID=4326;POLYGON(…))` in the same request. Both answer 200; the wrong one answers "no entity". `Axes` is a field of the provider, documented per parameter, and a test pins the decoded order of the built URL.
- **The quote comes first.** `count()` is an ordinary request (`resultType=hits`, about a kilobyte), not a mode: counts sit at the end of a response document, so without a separate request no cost can be announced before it is paid; it matches the library's plan-before-apply shape; and it makes a wrong-axis bug visible for a kilobyte instead of after a large harvest. A quote asks for no `outputFormat`: the service answers XML whatever is requested.
- **A server refusal is information.** A WFS answers errors as `ows:ExceptionReport` with HTTP 400 even when JSON was requested. The code and text come back in `Error::Server` verbatim, entities resolved. Two traps: an XML entity is an *event*, not text (the parser splits content around `&quot;`), and trimming each fragment eats the spaces around an entity; trim once, at the end.
- **What a provider declares is the form a UI renders.** A `Provider` declares its `Collection`s and, per collection, its `Filter`s with type, obligation, suggestion, a sentence saying what the filter is for, and the operations a harvest feeds (`feeds`). A UI builds the form without spelling a single field name. `Providers::catalogue()` is the single door, serialised as camelCase. A `Kind` travels tagged (`{"is":"oneOf","values":[…]}`), never recognised by its shape. A required filter must carry a suggestion, or the form is a trap; a test enforces it.
- **Adding a provider is one module and one line**: a module implementing `Provider` (for WFS, `wfs::Wfs` already encodes; only declarations and the measured axis order remain) and its line in `Providers::builtin()`, the only list. A caller's additions are tried **before** the built-ins, so a built-in is replaceable, not merely extendable.
- **A read is a stream.** `read_page` hands entities one at a time to a sink that decides to continue or stop; nothing of the document is held. Measured memory stays flat for tens of thousands of entities. `read()` is built on the streaming reader: one implementation, and a test confronts the two.
- **A page is a document; documents are not concatenated.** Writing harvested pages writes one file per page, numbered when there are several, by copying the stream without parsing it, since a round trip through a feature model would lose what it does not model (polygon inner rings).
- **A void is not a height, whatever the file declares.** Elevation rasters declare a no-data value and write another, and the service resamples, so pixels next to a void are blends no equality catches. `terrain::voids_of` treats as void the declared value (compared as the stored `f32`), everything below a terrestrial floor, and, only if a fractional value below the floor proves resampling, every pixel within a small reach of a void. The three causes are counted separately. A bathymetry raster needs another function, not a lower floor.
- **Not every provider is public, and none is authenticated here.** The form of a secret is unknown, so no mechanism is invented. If one arrives: the secret never goes in a page, never in a committed file, and a missing capability is declared (a provider that requires an identity without saying so would look like missing data). A VPN needs nothing here.

## Tests

`tests/wfs.rs`, `terrain.rs`, `ign_altimetrie.rs` never touch the network. Their fixtures are real responses captured from the services; an invented refusal would be a refusal we know how to read. Two catalogue tests keep the "form is the declaration" property: one plays a page (builds a valid request from the catalogue without naming any provider, collection or filter) and one adds a fake provider declared in the test and expects it in the catalogue untouched. Both were checked by sabotage.
