#!/usr/bin/env python3
"""Check the `# References` sections of the operations of `energyml-build`.

    python scripts/check-references.py

There is no reliable way to decide from the text alone that an operation
"describes an algorithm" (a keyword search flags operations that merely mention
kriging, or a spline, in passing), so this check does **not** demand a section
from every operation. It checks the sections that exist:

- a `# References` heading in a module doc (`//!`) of `crates/energyml-build/src/ops/*.rs`
  must be followed by at least one entry;
- an entry is either a citation (it holds a four-digit year, a DOI, a URL or a
  standard number such as `ISO`/`API`/`SPE`) or an open question written
  `TODO(reference)`, so a missing source is *said*, never silently absent.

Adding a new algorithm without a section is a review matter: the pull request
template asks for it. Exit status 0 when every section passes, 1 otherwise.
Standard library only.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

OPS = Path("crates/energyml-build/src/ops")
CITATION = re.compile(r"\b(?:1[89]|20)\d\d\b|doi\.org|\bdoi:|https?://|\b(?:ISO|API|SPE|IEC|ASTM|RFC)\b|TODO\(reference\)", re.I)


def main() -> int:
    if not OPS.is_dir():
        print(f"error: {OPS} not found; run this from the repository root", file=sys.stderr)
        return 2
    failures: list[str] = []
    sections = 0
    for path in sorted(OPS.glob("*.rs")):
        lines = path.read_text(encoding="utf-8").splitlines()
        for i, line in enumerate(lines):
            if not re.match(r"^//!\s*#+\s*References\s*$", line):
                continue
            sections += 1
            body: list[str] = []
            for nxt in lines[i + 1:]:
                if not nxt.startswith("//!"):
                    break
                text = nxt[3:].strip()
                if re.match(r"#+\s", text):
                    break
                if text:
                    body.append(text)
            if not body:
                failures.append(f"{path}:{i + 1}: `# References` is empty")
            elif not any(CITATION.search(b) for b in body):
                failures.append(f"{path}:{i + 1}: `# References` holds no citation (a year, DOI, URL, standard) and no TODO(reference)")
    for f in failures:
        print(f)
        print(f"::error file={f.split(':')[0]},line={f.split(':')[1]}::{f.split(': ', 1)[1]}")
    print(f"{sections} `# References` sections checked, {len(failures)} failure(s)")
    if sections == 0:
        print("error: no `# References` section found at all; the check would prove nothing", file=sys.stderr)
        return 1
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
