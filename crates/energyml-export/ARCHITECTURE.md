# energyml-export: architecture

## Purpose and position

Writes EnergyML geometry as OBJ, GeoJSON and VTK (three flavours) through a registry. Adding glTF or PLY is a new `Exporter` and nothing else. Exporters never see a store: they are handed a `SceneObject`. The crate is above `energyml-geometry`.

Related decisions: [ADR-0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR-0006](../../docs/adr/0006-registries-rather-than-match.md), [ADR-0012](../../docs/adr/0012-no-face-carries-logic.md).

## Rules

**Registries, not matches** — a trait, a registry, built-ins pushed in by default, and caller additions tried first, so a built-in can be replaced and not merely extended.

**GeoJSON refuses without a geodesy backend** — RFC 7946 is WGS 84 by definition. Writing projected eastings into the longitude field yields a file every tool loads and draws in the wrong ocean. The refusal names what is missing. The exporter takes a `&dyn Reprojector`; `energyml-proj` is the one shipped, and nothing here depends on it directly.

**Metadata is configuration, not code** — a `Spec` is a JSON map of key to dotted path, and a path crosses references transparently (`RepresentedInterpretation.InterpretedFeature.Citation.Title` walks two objects). One spec feeds every format: GeoJSON `properties`, VTK field data, OBJ comments. Adding a key is editing a file.

**A reference's cached title and the target's real one are both reachable, and different** — `X.Title` reads the reference's own cached field (free, possibly stale); `X.Citation.Title` resolves the object (one read, authoritative). Resolution is field-first, then resolve, so both stay addressable.

**A path that finds nothing is not an error, but a key that matched nowhere is reported** — a spec runs over a whole package where some objects have an interpretation and others do not; but a typo and a legitimately absent value look identical in the output, so a key that matched nowhere is named.

**Identity travels with the geometry** — a scene object carries uuid, ETP URI, title and the interpretation and feature above it. An exported surface that cannot be traced back to what it represents is a dead end, and tracing back is usually the reason for exporting.

## Deliberately not here

- No store access, no reading of arrays, no placement or reprojection decisions (the caller decides).
- No dependency on a geodesy library.

## How it is tested

Tests build scene objects by hand and read the output back (closed rings, undefined values, CSV and shape output, spec paths). The checks name what they would have to break to fail. Tests against local real packages are feature-gated and off by default.
