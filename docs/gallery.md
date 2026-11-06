# Gallery

Every image in this repository is **reproducible**: it is drawn from the
reference model [`energyml-lutetia`](https://github.com/geosiris-technologies/energyml-lutetia),
at the commit pinned in [`tests/lutetia.lock`](../tests/lutetia.lock), by the command
written beside it. The SVG files are what the command writes, with nothing edited
by hand; the same package gives the same bytes.

The commands below run from the folder that holds `package/Lutetia.epc` (the `.h5`
beside it is found by name). The three sections use one track, so the three views
show the same ground.

| Image | What it shows | Produced by |
|---|---|---|
| `images/section-long.svg` | long profile along a track: wells, horizons, faults, strata fills | `energyml profile package/Lutetia.epc --track "650700,6861450;651250,6861750;651850,6861350" --left 200 --right 150 --static --size 1400x700 --draw long --fill down --labels codes --svg section-long.svg` |
| `images/section-plan.svg` | map view of the same section | the same command with `--draw plan` and `--svg section-plan.svg`, without `--fill` and `--labels` |
| `images/section-cross.svg` | a cross section, the second of three | the same command with `--cross-count 3 --cross-length 300 --cross-width 100 --draw cross:2 --fill down` and `--svg section-cross.svg`, without `--labels` |
| `images/well-sheet.svg` | well sheet of one wellbore | TODO: `energyml-sheet` |
| `images/isocontours.svg` | iso-lines of a depth surface | TODO |
| `images/triangulation.svg` | constrained Delaunay triangulation of boreholes with a fault | TODO |
| `images/kriging.svg` | kriged surface and its variance | TODO |

The long section and the cross section carry notes under their plot, as every
drawing does: how many objects were refused (a surface in time with no time factor,
for example) and how many wells fall outside the band. Nothing is hidden to make the
picture tidier.

![Long section of Lutetia](images/section-long.svg)

![Plan view of the same section](images/section-plan.svg)

![Cross section of Lutetia](images/section-cross.svg)

Screenshots of interactive applications are not part of this repository.
