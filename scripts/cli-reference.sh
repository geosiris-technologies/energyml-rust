#!/usr/bin/env bash
# The CLI reference, `docs/cli-reference.md`, written from the command
# definitions (`energyml cli-reference`) -- or checked, with `--check`.
#
# Built with `hdf5` and `png`: the reference documents the options they add.
# The command leaves hidden commands out whatever the build; the features here
# decide which options appear, so the page is the same on every machine that
# runs this script. HDF5 needs its C library at build time (libhdf5-dev on
# Debian/Ubuntu).
#
#   bash scripts/cli-reference.sh           # write it
#   bash scripts/cli-reference.sh --check   # fail when it is stale (CI)
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run -q -p energyml-cli --features hdf5,png -- cli-reference --out docs/cli-reference.md "$@"
