# Public data sources

`energyml-providers` queries **public APIs only** and turns what they answer into
input for the [operations engine](creating-objects.md). It never needs a secret.
Check each provider's own terms before reusing data: the licence column below is
what the shipped catalogue states, not legal advice.

## Principle

> Building a request and reading a response are pure. The round trip is not.

Everything that can be wrong - encoding a filter, axis order of a bounding box,
reading a server's refusal - is pure and tested without a network. The round trip
is a `Transport` trait; the native implementation (`ureq`) is behind the `native`
feature, and a browser would supply its own `fetch`.

A provider **declares** what it offers: its collections and, for each, the
filters it accepts with their type, whether they are required and a suggested
value. A command-line tool or a form is drawn from that declaration without
spelling a provider, a collection or a filter name.

Axis order matters and was measured: the same WFS wants `lat, lon` in a `bbox`
and `lon, lat` in a CQL polygon, and the wrong one answers "nothing here"
without complaining. That is each provider's business and is held in its
definition.

```sh
energyml sources                                  # what this build can query
energyml sources brgm-bss                         # collections, filters, suggestions
energyml fetch brgm-bss boreholes --area area.geojson --where codeSpace=BRGM        # the quote
energyml fetch brgm-bss boreholes --area area.geojson --where codeSpace=BRGM --out bss.json
energyml terrain --list --area 651000,6860000,653000,6862000
energyml terrain --area 651000,6860000,653000,6862000 --step 25 --set vertical=5720 --out dem.epc
```

`fetch` and `terrain` quote first and download only on request. Reading is
streamed, so a harvest of tens of thousands of features stays within tens of
megabytes of memory.

## Boreholes: the BSS

| | |
|---|---|
| Provider | `brgm-bss`: boreholes of the French *Banque du Sous-Sol* (BSS), published by the BRGM through a WFS, GeoSciML 4.1 `Borehole`, plus intervals |
| Endpoint | `https://data.geoscience.fr/api/gsmlbh/ows` |
| Attribution | BRGM - Banque du sous-sol (BSS) |
| Licence | **not stated.** The service declares no fees and no access constraints, which is not the same as stating a licence; the catalogue therefore shows none rather than inventing a credit. TODO(verify): confirm the licence with the data provider |

A harvested page feeds the import operation its collection declares: `energyml create --list --from bss.json`, then `energyml create sigeol-boreholes --from bss.json --set project=2154 --out bss.epc`.
(The operation reads GeoSciML; it is tested with SIGEOL exports.)

## Terrain and imagery catalogue

Sources are a **catalogue**, not a list in code: each entry states its protocol,
coverage boxes, systems, resolution, licence and attribution
(`crates/energyml-providers/catalogue/terrain.json`). A protocol is code; a
source is data - another service speaking a protocol already read is one more
entry, no rebuild. `--catalogue mine.json` puts your entries first; one with the
same `id` replaces the shipped one.

| Id | What | Resolution | Licence | Read today |
|---|---|---|---|---|
| `ign-rge-alti` | IGN RGE ALTI, bare-earth terrain model (France), WMS raster | 1 m | Licence Ouverte / Etalab 2.0 | yes |
| `ign-lidar-hd-mnt` | IGN LiDAR HD, bare-earth model | 0.5 m | Licence Ouverte / Etalab 2.0 | yes (coverage partial, growing) |
| `ign-rge-alti-mns` | IGN RGE ALTI surface model (buildings, vegetation) | 1 m | Licence Ouverte / Etalab 2.0 | yes |
| `ign-ortho` | IGN orthophotos (BD ORTHO), WMTS | 0.2 m | Licence Ouverte / Etalab 2.0 | imagery: listed, not turned into terrain |
| `copernicus-dem-30` | Copernicus DEM GLO-30 (cloud-optimised GeoTIFF on a public bucket) | 30 m | Copernicus DEM licence | listed; protocol not read yet |
| `aws-terrain-tiles` | AWS Terrain Tiles (terrarium PNG), worldwide | about 30 m | mixed open data (SRTM, 3DEP and others); see the attribution document of the tiles | listed; protocol not read yet |
| `s2cloudless-2016` | EOX Sentinel-2 cloudless 2016, WMTS | 10 m | CC BY 4.0 | imagery: listed |

Attributions as shipped:

- IGN sources: `IGN - RGE ALTI`, `IGN - LiDAR HD`, `IGN - BD ORTHO`.
- Copernicus: "(c) DLR e.V. 2021 and (c) Airbus Defence and Space GmbH 2021
  provided under COPERNICUS by the European Union and ESA".
- AWS Terrain Tiles: see <https://github.com/tilezen/joerd/blob/master/docs/attribution.md>.
- EOX: "Sentinel-2 cloudless - https://s2maps.eu by EOX IT Services GmbH
  (Contains modified Copernicus Sentinel data 2016 & 2017)".

The grid a `terrain` run writes carries its source, service, layer, licence,
attribution and fetch date in the activity and the object's description. Without
`--source`, the finest relief source that covers the area **and whose protocol is
read** is chosen; imagery, an unread protocol, an area nothing covers and a window
beyond the service's size limit (5010 pixels a side for the IGN WMS) are refused
by name, the last with the step that would fit.

Coverage boxes are approximate: a box inside a source's envelope is not a promise
of data (the IGN layer's envelope includes overseas territories, sea and foreign
land).

## Adding a provider

An API speaking a protocol already read (WFS, WMS raster) is an entry in a
declaration, not a code change. A new protocol is a `Provider` implementation
behind the `Transport` trait. Either way the new provider appears in
`energyml sources` and in any form built from the catalogue.

TODO(verify): provider list and catalogue entries at the time of the first
release.
