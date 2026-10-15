#!/bin/sh
# Run a handful of `energyml` commands on the Lutetia reference model, end to end.
#
#   sh examples/cli/lutetia.sh [LUTETIA_DIR]
#
# Needs: an `energyml` binary on the PATH (built with the `hdf5` and `png` features for
# every step; see README.md in this folder), and either `git` or `curl`.
# LUTETIA_DIR is where Lutetia is cloned or downloaded (default: ./lutetia). If it already
# holds package/Lutetia.epc, nothing is fetched.
# Everything the commands write goes under ./out/.
set -eu

BASE=https://raw.githubusercontent.com/geosiris-technologies/energyml-lutetia/main
REPO=https://github.com/geosiris-technologies/energyml-lutetia
DIR=${1:-lutetia}
OUT=out

command -v energyml >/dev/null 2>&1 || {
  echo "energyml is not on the PATH: see examples/cli/README.md" >&2
  exit 2
}

# 1. Get Lutetia (the EPC and its HDF5 file must stay in the same folder).
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
EPC="$DIR/package/Lutetia.epc"
mkdir -p "$OUT"

# 2. Look at it.
echo "== list"
energyml list "$EPC" | head -n 15
echo "== check (the clean package: exit 0)"
energyml check "$EPC" --errors-only
echo "== arrays"
energyml arrays "$EPC" --summary
echo "== geometry"
energyml geometry "$EPC" | head -n 20

# 3. Export.
echo "== export"
energyml export "$EPC" --list
energyml export "$EPC" --format obj -o "$OUT/obj"
energyml export "$EPC" --format geojson -o "$OUT/geojson"
energyml graph "$EPC" --format dot -o "$OUT/graph.dot"
energyml wells "$EPC" --out "$OUT/wells.csv"

# 4. A section view along the trace of the recipe (Lambert-93 metres).
echo "== profile"
TRACK="650700,6861450;651250,6861750;651850,6861350"
energyml profile "$EPC" --track "$TRACK" --left 200 --right 150 --cross-count 3 --draw long --svg "$OUT/long.svg" --png "$OUT/long.png"
energyml profile "$EPC" --track "$TRACK" --left 200 --right 150 --draw plan --svg "$OUT/plan.svg"

# 5. The variant with deliberate errors: check exits 1, which is the point.
if [ -f "$DIR/package-with-errors/Lutetia.epc" ]; then
  echo "== check on the package with errors (exit 1 expected)"
  status=0
  energyml check "$DIR/package-with-errors/Lutetia.epc" --errors-only || status=$?
  echo "exit code: $status"
fi

# 6. Play the recipe (only with a clone: the recipe reads files next to it).
if [ -f "$DIR/recipe/recette.toml" ]; then
  echo "== replay the recipe into $OUT/replayed/"
  energyml create --recipe "$DIR/recipe/recette.toml" --out "$OUT/replayed/Lutetia.epc"
fi

echo "done: see ./$OUT/"
