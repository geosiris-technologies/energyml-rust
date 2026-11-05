#!/usr/bin/env bash
# Decides whether a change needs the build jobs of CI.
#
# Reads the changed file names on stdin, one per line, and prints `code=true` or
# `code=false` (the form GITHUB_OUTPUT wants). `false` only when EVERY file is
# documentation that no build, test or check reads. Anything unknown, and an empty
# list, is `true`: a doubt always runs the jobs.
#
# Documentation, and nothing else:
#   - a root-level *.md (README, CONTRIBUTING, SECURITY, ARCHITECTURE, ...);
#   - anything under docs/, except docs/cli-reference.md, which the `cli-reference`
#     job compares with what the commands say;
#   - a crate's README.md or CHANGELOG.md (cargo packs them, nothing compiles them);
#   - a Markdown file under examples/;
#   - CITATION.cff.
#
# Used by the `changes` job of .github/workflows/ci.yml; tested by
# scripts/ci-changes.test.sh.
set -euo pipefail

is_doc() {
  case "$1" in
    docs/cli-reference.md) return 1 ;;
    docs/*) return 0 ;;
    CITATION.cff) return 0 ;;
    */*) ;;
    *.md) return 0 ;;
  esac
  case "$1" in
    crates/*/README.md | crates/*/CHANGELOG.md) return 0 ;;
    examples/*.md) return 0 ;;
  esac
  return 1
}

seen=0
code=false
while IFS= read -r file; do
  [ -z "$file" ] && continue
  seen=1
  if ! is_doc "$file"; then
    code=true
  fi
done

if [ "$seen" -eq 0 ]; then
  code=true
fi
echo "code=$code"
