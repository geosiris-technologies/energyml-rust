#!/bin/sh
# The same tour of Lutetia as lutetia.sh, with the Docker image: nothing to install but Docker.
#
#   sh examples/cli/lutetia-docker.sh [IMAGE]
#
# Needs: docker, and either `git` or `curl` to fetch Lutetia (the image has no network
# step of its own). Run it from a scratch folder: it creates ./lutetia and ./out there,
# and the current folder is mounted at /work in the container.
# IMAGE defaults to ghcr.io/geosiris-technologies/energyml:latest.
set -eu

IMG=${1:-ghcr.io/geosiris-technologies/energyml:latest}
BASE=https://raw.githubusercontent.com/geosiris-technologies/energyml-lutetia/main
REPO=https://github.com/geosiris-technologies/energyml-lutetia
DIR=lutetia
OUT=out

command -v docker >/dev/null 2>&1 || {
  echo "docker is not installed" >&2
  exit 2
}

if [ ! -f "$DIR/package/Lutetia.epc" ]; then
  if command -v git >/dev/null 2>&1; then
    git clone --depth 1 "$REPO" "$DIR"
  elif command -v curl >/dev/null 2>&1; then
    mkdir -p "$DIR/package"
    curl -fsSL -o "$DIR/package/Lutetia.epc" "$BASE/package/Lutetia.epc"
    curl -fsSL -o "$DIR/package/Lutetia.h5" "$BASE/package/Lutetia.h5"
  else
    echo "need git or curl to fetch Lutetia" >&2
    exit 2
  fi
fi
mkdir -p "$OUT"

# The image runs as uid 1000; --user makes the files it writes belong to you.
en() {
  docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/work" "$IMG" "$@"
}

EPC="$DIR/package/Lutetia.epc"

echo "== list"
en list "$EPC" | head -n 15
echo "== check (the clean package: exit 0)"
en check "$EPC" --errors-only
echo "== arrays"
en arrays "$EPC" --summary
echo "== geometry"
en geometry "$EPC" | head -n 20

echo "== export"
en export "$EPC" --list
en export "$EPC" --format obj -o "$OUT/obj"
en export "$EPC" --format geojson -o "$OUT/geojson"
en graph "$EPC" --format dot -o "$OUT/graph.dot"
en wells "$EPC" --out "$OUT/wells.csv"

echo "== profile"
TRACK="650700,6861450;651250,6861750;651850,6861350"
en profile "$EPC" --track "$TRACK" --left 200 --right 150 --cross-count 3 --draw long --svg "$OUT/long.svg" --png "$OUT/long.png"
en profile "$EPC" --track "$TRACK" --left 200 --right 150 --draw plan --svg "$OUT/plan.svg"

if [ -f "$DIR/package-with-errors/Lutetia.epc" ]; then
  echo "== check on the package with errors (exit 1 expected)"
  status=0
  en check "$DIR/package-with-errors/Lutetia.epc" --errors-only || status=$?
  echo "exit code: $status"
fi

if [ -f "$DIR/recipe/recette.toml" ]; then
  echo "== replay the recipe into $OUT/replayed/"
  en create --recipe "$DIR/recipe/recette.toml" --out "$OUT/replayed/Lutetia.epc"
fi

echo "done: see ./$OUT/"
