# The `energyml` image

Everything the `energyml` command does, without compiling anything: the CLI built
with HDF5, in a container, working on the files of a mounted folder.

## Get it

```sh
docker pull ghcr.io/geosiris-technologies/energyml:latest
```

`edge` follows the main branch. `latest` is the newest release; every release is also published as `X.Y.Z` and `X.Y`
(for example `0.1.0`, `0.1`), for a chain that must not move under its feet. Images are built for amd64 and arm64 by the CI of the repository, never by hand.

Or build it from a checkout:

```sh
docker build -f docker/Dockerfile -t energyml .
```

`--build-arg RELEASE_LTO=thin` builds faster and with less memory.

## Use it

The folder holding your files is mounted at `/work`, which is the working
directory, so paths are written relative to it:

```sh
IMG=ghcr.io/geosiris-technologies/energyml:latest
docker run --rm -v "$PWD:/work" $IMG list model.epc
docker run --rm -v "$PWD:/work" $IMG check model.epc
docker run --rm -v "$PWD:/work" $IMG export model.epc --format obj -o out
docker run --rm -v "$PWD:/work" $IMG create --recipe recipe.toml --out model.epc
```

An EPC and its `.h5` must both be inside the mounted folder: the package names its
data file relatively.

PowerShell users write `-v "${PWD}:/work"`.

## What the image holds

| Path | What |
|---|---|
| `/usr/local/bin/energyml` | the CLI, built with HDF5 and the PNG rasteriser of the section view |
| `/usr/local/bin/enyml` | the second binary built by the CLI crate; the entry point runs it as it is |
| `/usr/local/bin/energyml-mcp` | the MCP server over a package ([MCP server](mcp.md)), from the release after 0.2.0; run it with `docker run -i <image> energyml-mcp model.epc` |
| `/opt/energyml/scripts/galaxy/energyml-galaxy.py` | the runner of the generated [Galaxy tools](galaxy.md) |
| `/opt/energyml/REF` | the commit the image was built from |

The image runs as uid 1000, not root. On a Linux host, files it writes into the
mounted folder belong to that user; Docker Desktop maps them to your user.

The ETP client (`retp`) is not in this image: `energyml etp ...` says which program to install, and `retp` publishes its own image, `ghcr.io/geosiris-technologies/retp`.


## Logging

```sh
docker run --rm -e ENERGYML_LOG=info -v "$PWD:/work" $IMG create --recipe recipe.toml --out out.epc
```

See [Logging](logging.md).

## Running under a workflow engine

The entry point is `energyml`, or the program given in its place: a first
argument that is a path (`/bin/sh`, `./run.sh`) or `sh`, `bash`, `python`, `python3`, `env`, `enyml`
is run as it is; with no argument it prints the help. Galaxy runs a tool's command as
`docker run <image> /bin/sh -c "<command>"`, which an `energyml`-only entry point
would refuse as an unknown subcommand. See [Galaxy tools](galaxy.md).
