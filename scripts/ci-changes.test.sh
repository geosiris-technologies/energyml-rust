#!/usr/bin/env bash
# Tests scripts/ci-changes.sh. Run: bash scripts/ci-changes.test.sh
# Each case is the changed files (one per line) and the answer expected.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
check() {
  local want=$1 name=$2 files=$3 got
  got=$(printf '%s\n' "$files" | bash scripts/ci-changes.sh)
  if [ "$got" != "code=$want" ]; then
    echo "FAIL $name: wanted code=$want, got $got"
    fail=1
  fi
}

check false "root readme" "README.md"
check false "docs page" "docs/library.md"
check false "docs image" "docs/images/hero-section.svg"
check false "crate readme" "crates/energyml/README.md"
check false "crate changelog" "crates/energyml-cli/CHANGELOG.md"
check false "example readme" "examples/cli/README.md"
check false "several docs" $'README.md\ndocs/docker.md\ncrates/energyml/README.md\nexamples/cli/README.md'

check true "rust source" "crates/energyml/src/lib.rs"
check true "cargo manifest" "crates/energyml/Cargo.toml"
check true "lockfile" "Cargo.lock"
check true "cli reference" "docs/cli-reference.md"
check true "workflow" ".github/workflows/ci.yml"
check true "script" "scripts/ci-changes.sh"
check true "data note in a crate" "crates/energyml-domain/data/property_kinds.NOTICE.md"
check true "NOTICE" "NOTICE"
check true "one code file among docs" $'README.md\ndocs/docker.md\ncrates/energyml/src/lib.rs'
check true "empty list" ""

[ "$fail" -eq 0 ] && echo "ci-changes: all cases pass"
exit "$fail"
