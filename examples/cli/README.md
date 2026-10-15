# Command line examples

Copy-and-run examples for the `energyml` command and its Docker image, on the public
reference model Lutetia. Every command and flag below is listed in
[`docs/cli-reference.md`](../../docs/cli-reference.md); the how and the why are in
[`docs/cli.md`](../../docs/cli.md).

| File | What |
|---|---|
| `lutetia.sh` | a short end-to-end tour with a local `energyml` (POSIX `sh`) |
| `lutetia-docker.sh` | the same tour with the Docker image, nothing to install |
| `lutetia.ps1` | the same tour in PowerShell |

Run one from any scratch folder, for example `sh examples/cli/lutetia.sh`. It fetches
Lutetia into `./lutetia` and writes into `./out/`.

## Get the CLI

Pick one.

- **A release archive.** Download `energyml-vX.Y.Z-<target>.tar.gz` (or `.zip` on
  Windows) from the [releases page](https://github.com/geosiris-technologies/energyml-rust/releases),
  unpack it and put `energyml` on your `PATH`. The Linux (glibc), macOS and Windows archives
  are built with HDF5 linked in, so arrays can be read; the Linux musl archives are built
  without it. No archive has the `png` feature: `profile --png` needs the image or a build
  from source.
- **From source:** `cargo install energyml-cli --features hdf5,png` (needs the HDF5 C library at
  build time; drop the features for a build without array reading and PNG).
- **Docker:** `docker pull ghcr.io/geosiris-technologies/energyml:latest` (tags `latest`,
  `0.2.0`, `0.2`, `edge`). It holds the CLI built with HDF5 and PNG. See
  [`docs/docker.md`](../../docs/docker.md).

## Get Lutetia

```sh
git clone --depth 1 https://github.com/geosiris-technologies/energyml-lutetia
```

or download the two files of a variant into one folder (the EPC names its HDF5 file, so
they must stay together):

```sh
mkdir -p lutetia/package
B=https://raw.githubusercontent.com/geosiris-technologies/energyml-lutetia/main
curl -fsSL -o lutetia/package/Lutetia.epc $B/package/Lutetia.epc
curl -fsSL -o lutetia/package/Lutetia.h5  $B/package/Lutetia.h5
```

`package/` is the clean model; `package-with-errors/` has deliberate errors for testing
checkers (the two variants share file names: keep each pair in its own folder).

## Docker in one line

The folder holding your files is mounted at `/work`, the working directory, so paths are
relative to it. Every docker command below is the native one with this prefix:

```sh
IMG=ghcr.io/geosiris-technologies/energyml:latest
docker run --rm -v "$PWD:/work" $IMG list lutetia/package/Lutetia.epc
```

PowerShell: write `-v "${PWD}:/work"`. On Linux add `--user "$(id -u):$(id -g)"` if you want the files
the image writes to belong to you (it runs as uid 1000).

In the sections below, `energyml ARGS` and `docker run --rm -v "$PWD:/work" $IMG ARGS` are
interchangeable.

## Look at a package

```sh
energyml list lutetia/package/Lutetia.epc                  # uuid, type, title
energyml list lutetia/package/Lutetia.epc --filter grid    # only types containing "grid"
energyml check lutetia/package/Lutetia.epc                 # package, objects, references
energyml arrays lutetia/package/Lutetia.epc --summary      # where the numbers live
energyml geometry lutetia/package/Lutetia.epc              # what can be drawn, CRS, point counts
energyml properties lutetia/package/Lutetia.epc --check    # what the numbers mean
energyml colors lutetia/package/Lutetia.epc                # the colour of each object, and why
```

```sh
docker run --rm -v "$PWD:/work" $IMG list lutetia/package/Lutetia.epc
docker run --rm -v "$PWD:/work" $IMG check lutetia/package/Lutetia.epc
docker run --rm -v "$PWD:/work" $IMG arrays lutetia/package/Lutetia.epc --summary
```

One object, by uuid (take one from `list`):

```sh
energyml show lutetia/package/Lutetia.epc <uuid> --json
```

The reference graph, as Graphviz or JSON, and the wells as a table:

```sh
energyml graph lutetia/package/Lutetia.epc --format dot -o graph.dot
energyml wells lutetia/package/Lutetia.epc --out wells.csv
```

TODO(verify): the object counts and the exact output text of these commands on Lutetia; the
inventory is in `docs/inventory.md` of the Lutetia repository.

## Check, and the package with errors

```sh
energyml check lutetia/package/Lutetia.epc --errors-only             # exit 0
energyml check lutetia/package-with-errors/Lutetia.epc --errors-only # exit 1
energyml check lutetia/package-with-errors/Lutetia.epc --no-schema   # skip the slow schema pass
echo $?
```

The expected findings are described in `docs/errors.md` of the Lutetia repository.

## Export

```sh
energyml export lutetia/package/Lutetia.epc --list                                  # formats of this build
energyml export lutetia/package/Lutetia.epc --format obj -o out/obj                 # one file per representation
energyml export lutetia/package/Lutetia.epc --format geojson -o out/geojson         # always WGS 84
energyml export lutetia/package/Lutetia.epc --format vtu --properties -o out/vtu    # with property values (needs hdf5)
energyml export lutetia/package/Lutetia.epc --format obj --as '*=points' -o out/pts # force a form
```

```sh
docker run --rm -v "$PWD:/work" $IMG export lutetia/package/Lutetia.epc --format obj -o out/obj
```

A representation that cannot be written is named with the reason and does not stop the
others. Files are named `qualifiedType_uuid.ext`.

## Section view of Lutetia

A long profile along a two-segment trace (Lambert-93 metres, the CRS of Lutetia), drawn as
SVG and PNG, then the map view and one cross section. These are the options used to draw
the picture shown in the Lutetia README.

```sh
TRACK="650700,6861450;651250,6861750;651850,6861350"
energyml profile lutetia/package/Lutetia.epc --track "$TRACK" --left 200 --right 150 \
  --cross-count 3 --cross-length 300 --cross-width 100 --draw long --size 1400x700 \
  --labels full --svg out/long.svg --png out/long.png --png-scale 1.5
energyml profile lutetia/package/Lutetia.epc --track "$TRACK" --left 200 --right 150 --draw plan --svg out/plan.svg
energyml profile lutetia/package/Lutetia.epc --track "$TRACK" --left 200 --right 150 \
  --cross-count 3 --cross-length 300 --cross-width 100 --draw cross:2 --svg out/cross-2.svg
```

A marker on the BSS boreholes, as in the Lutetia recipe:

```sh
energyml profile lutetia/package/Lutetia.epc --track "$TRACK" --left 200 --right 150 --draw long \
  --marker "Forages BSS:shape=diamond,colour=#1565c0,at=bottom,type=WellboreTrajectory,title=BSS" \
  --svg out/long-markers.svg
```

With Docker, the same with the prefix (`--png` works: the image has the PNG rasteriser):

```sh
docker run --rm -v "$PWD:/work" $IMG profile lutetia/package/Lutetia.epc \
  --track "650700,6861450;651250,6861750;651850,6861350" --left 200 --right 150 \
  --svg out/long.svg --png out/long.png
```

Without `--svg`, `profile` only lists which wells fall in the band (`--json` for the full
answer). See [`docs/section-view.md`](../../docs/section-view.md).

## Create objects from a recipe

`energyml create` writes objects through one door; a recipe is a TOML file that chains the
steps. The recipe that builds Lutetia is in the Lutetia clone:

```sh
energyml create --list                                          # what can be made from nothing
energyml create crs --set epsg=2154 --set vertical=5720 --plan  # the quote, nothing written
energyml create crs --set epsg=2154 --set vertical=5720 --out out/crs.epc
energyml create --recipe lutetia/recipe/recette.toml --out out/Lutetia.epc   # replays Lutetia (needs hdf5)
```

`--plan` quotes one operation. A whole recipe is played rather than quoted: its later
steps read what the earlier ones wrote, which a quote does not write.

See [`docs/creating-objects.md`](../../docs/creating-objects.md).

## The MCP server

`energyml-mcp` serves a package, read-only, to an AI assistant over stdin/stdout (see
[`docs/mcp.md`](../../docs/mcp.md)). The release archives hold `energyml` only; install it
with:

```sh
cargo install energyml-mcp-cli --features hdf5
```

The Docker image ships it from the release after 0.2.0 (the 0.2.0 image has `energyml`
only). An MCP client then starts it with stdin kept open (`-i`) and the package's folder
mounted:

```sh
docker run -i --rm -v "$PWD/lutetia/package:/work" ghcr.io/geosiris-technologies/energyml:latest energyml-mcp Lutetia.epc
```

An MCP client launches it itself. Claude Code:

```sh
claude mcp add lutetia -- energyml-mcp /path/to/lutetia/package/Lutetia.epc
```

Claude Desktop (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "lutetia": {
      "command": "energyml-mcp",
      "args": ["/path/to/lutetia/package/Lutetia.epc"]
    }
  }
}
```

Running `energyml-mcp file.epc` in a terminal just waits for a client. Nothing is written
to the package.

## Exit codes

- `check` exits 1 when it finds errors, 0 otherwise.
- A command that needs a capability the build lacks (for example `arrays --values` or an
  export with `--properties` without `hdf5`, `profile --png` without `png`) refuses and says
  which feature is missing; it never prints an empty result instead.
- TODO(verify): the exit code of the other commands on a refusal (usage errors are the
  usual non-zero code of the argument parser); `docs/cli.md` documents only the `check` case.
- The scripts here stop at the first failing command (`set -eu`), except the check of the
  package with errors, where exit 1 is the expected result.
