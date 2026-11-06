#!/usr/bin/env python3
"""List every TODO of the Markdown files of the repository.

Run from anywhere:  python scripts/list-todos.py [--count] [PATH ...]

Reads the files git tracks (so target/ and the like are never scanned), or the
paths given. Prints `file:line: text`, grouped by file, and a total. With
`--count` only the number per file is printed.

A TODO is the word in capitals: `TODO`, `TODO(verify)`, `TODO(reference)`. The
three kinds mean different things (a fact to check, a citation to add, work to
do), so each line keeps its text and the summary counts them apart.
"""
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

PATTERN = re.compile(r"TODO(?:\(([^)]*)\))?")


def markdown_files(root: Path, given: list[str]) -> list[Path]:
    if given:
        return [Path(p) for p in given]
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.md"],
        cwd=root,
        capture_output=True,
        check=True,
    ).stdout.decode("utf-8")
    return [root / name for name in listed.split("\0") if name]


def main() -> int:
    args = sys.argv[1:]
    count_only = "--count" in args
    given = [a for a in args if a != "--count"]
    root = Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], capture_output=True, check=True
        )
        .stdout.decode("utf-8")
        .strip()
    )
    kinds: Counter = Counter()
    per_file: dict[str, list[tuple[int, str]]] = {}
    for path in markdown_files(root, given):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except OSError as error:
            print(f"cannot read {path}: {error}", file=sys.stderr)
            return 2
        for number, line in enumerate(lines, 1):
            for match in PATTERN.finditer(line):
                kinds[match.group(1) or "plain"] += 1
                name = path.relative_to(root).as_posix() if path.is_absolute() else str(path)
                per_file.setdefault(name, []).append((number, line.strip()))
                break  # one entry per line, whatever the number of TODOs on it
    for name in sorted(per_file):
        entries = per_file[name]
        if count_only:
            print(f"{len(entries):4}  {name}")
            continue
        print(f"\n{name} ({len(entries)})")
        for number, text in entries:
            shown = text if len(text) <= 160 else text[:157] + "..."
            print(f"  {number}: {shown}")
    total = sum(len(v) for v in per_file.values())
    summary = ", ".join(f"{n} {k}" for k, n in sorted(kinds.items()))
    print(f"\n{total} line(s) with a TODO in {len(per_file)} file(s)" + (f" ({summary})" if summary else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
