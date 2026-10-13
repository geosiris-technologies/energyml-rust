# Gallery

Every image in this repository is **reproducible**: it is produced from the
reference model [`energyml-lutetia`](https://github.com/geosiris-technologies/energyml-lutetia)
by one command, and CI fails when an image is stale.

```sh
cargo run --example gallery               # writes every image under docs/images/
cargo run --example gallery -- --check    # fails if an image differs from what the code draws
```

TODO: `examples/gallery.rs` does not exist yet. Until it does, no image is
committed and the list below is a placeholder. Each entry will carry the exact
command (or call) that produces it.

| Image | What it shows | Produced by |
|---|---|---|
| `images/section-long.svg` | long profile along a track: wells, horizons, faults, strata fills | TODO: `energyml profile ... --draw long` |
| `images/section-plan.svg` | map view of the same section | TODO: `energyml profile ... --draw plan` |
| `images/section-cross.svg` | a cross section | TODO: `energyml profile ... --draw cross:N` |
| `images/well-sheet.svg` | well sheet of one wellbore | TODO: `energyml-sheet` |
| `images/isocontours.svg` | iso-lines of a depth surface | TODO |
| `images/triangulation.svg` | constrained Delaunay triangulation of boreholes with a fault | TODO |
| `images/kriging.svg` | kriged surface and its variance | TODO |

Screenshots of interactive applications are not part of this repository.
