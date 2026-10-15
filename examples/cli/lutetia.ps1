# Run a handful of `energyml` commands on the Lutetia reference model (Windows PowerShell).
#
#   pwsh examples/cli/lutetia.ps1 [-Dir lutetia]
#
# Needs: energyml.exe on the PATH (a release archive, see README.md in this folder) and git.
# Outputs go under .\out\.
param([string]$Dir = "lutetia")
$ErrorActionPreference = "Stop"

if (-not (Get-Command energyml -ErrorAction SilentlyContinue)) {
    Write-Error "energyml is not on the PATH: see examples/cli/README.md"
}
if (-not (Test-Path "$Dir/package/Lutetia.epc")) {
    git clone --depth 1 https://github.com/geosiris-technologies/energyml-lutetia $Dir
}
$epc = "$Dir/package/Lutetia.epc"
New-Item -ItemType Directory -Force out | Out-Null

Write-Host "== list"
energyml list $epc | Select-Object -First 15
Write-Host "== check (the clean package: exit 0)"
energyml check $epc --errors-only
Write-Host "== arrays"
energyml arrays $epc --summary
Write-Host "== geometry"
energyml geometry $epc | Select-Object -First 20

Write-Host "== export"
energyml export $epc --list
energyml export $epc --format obj -o out/obj
energyml export $epc --format geojson -o out/geojson
energyml graph $epc --format dot -o out/graph.dot
energyml wells $epc --out out/wells.csv

Write-Host "== profile"
$track = "650700,6861450;651250,6861750;651850,6861350"
energyml profile $epc --track $track --left 200 --right 150 --cross-count 3 --draw long --svg out/long.svg --png out/long.png
energyml profile $epc --track $track --left 200 --right 150 --draw plan --svg out/plan.svg

if (Test-Path "$Dir/package-with-errors/Lutetia.epc") {
    Write-Host "== check on the package with errors (exit 1 expected)"
    energyml check "$Dir/package-with-errors/Lutetia.epc" --errors-only
    Write-Host "exit code: $LASTEXITCODE"
}

if (Test-Path "$Dir/recipe/recette.toml") {
    Write-Host "== replay the recipe into out/replayed/"
    energyml create --recipe "$Dir/recipe/recette.toml" --out out/replayed/Lutetia.epc
}
Write-Host "done: see .\out\"
