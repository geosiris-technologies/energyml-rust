#!/usr/bin/env python3
"""The round trip of a set of boreholes: CSV → EPC → CSV, cell by cell.

    python scripts/roundtrip-wells.py before.csv after.csv --template osdu_cols.json

Written for the check requested on 22/09: *"run a well export on an EPC that
comes from an import, then import the exported file to compare it with the
starting file [...] check that the properties also have the right structures
and values, besides checking the md"*.

# What it compares, and what it refuses to compare

It pairs **by (borehole, measured depth)**, never by row rank: the starting file
is not sorted, the one that comes back is, and comparing two lists rank by rank
would say *everything is wrong* of a perfect round trip.

It only compares the columns that the **template** names. The other 52 were
never written into the package - the operation says so (`columns-ignored`) and
it is not a loss to discover here.

# What it tolerates, and why each tolerance is named

- **how a number is written.** `39.0` comes back as `39`: a double has no
  canonical form, and demanding the string would fail an exact round trip. Two
  cells that both read as numbers are compared **as numbers**, within 1e-9;
- **an empty cell.** The file may say nothing where the package writes an
  absence, and both say the same thing.

Everything else is a difference, and it is **named with its two values**: a
count alone would not say which column moved.
"""

from __future__ import annotations

import argparse
import csv
import io
import json
import sys
from collections import defaultdict
from pathlib import Path


def read(path: Path) -> list[dict[str, str]]:
    """The rows, separator sniffed on the header and outside quotes."""
    raw = path.read_bytes()
    for encoding in ("utf-8-sig", "utf-8", "cp1252"):
        try:
            text = raw.decode(encoding)
            break
        except UnicodeDecodeError:
            continue
    else:
        raise SystemExit(f"{path}: neither UTF-8 nor cp1252")
    first = text.splitlines()[0] if text.splitlines() else ""
    best, most = ",", 0
    for candidate in (";", ",", "\t"):
        quoted, count = False, 0
        for char in first:
            if char == '"':
                quoted = not quoted
            elif char == candidate and not quoted:
                count += 1
        if count > most:
            best, most = candidate, count
    return list(csv.DictReader(io.StringIO(text), delimiter=best))


def number(text: str) -> float | None:
    try:
        return float(text.replace(",", "."))
    except (ValueError, AttributeError):
        return None


def same(a: str, b: str) -> bool:
    """Do two cells say the same thing?"""
    a, b = (a or "").strip(), (b or "").strip()
    if a == b:
        return True
    if not a or not b:
        # An absence on both sides: the file says nothing, the package writes a
        # null, and both mean *no measurement here*.
        return not a and not b
    x, y = number(a), number(b)
    if x is not None and y is not None:
        return abs(x - y) <= 1e-9 * max(1.0, abs(x), abs(y))
    return False


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")

    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("before", help="the starting CSV")
    parser.add_argument("after", help="the CSV that comes back from the EPC")
    parser.add_argument("--template", required=True, help="the OSDU template, which says which columns count")
    parser.add_argument("--well-column", default="SONDAGE")
    parser.add_argument("--md-column", default="PROF")
    parser.add_argument("--show", type=int, default=10)
    args = parser.parse_args()

    template = json.loads(Path(args.template).read_text(encoding="utf-8"))
    if isinstance(template, dict):
        template = template.get("data", {}).get("Columns", template)
    declared = [one.get("ColumnName") for one in template if one.get("ColumnName")]

    before, after = read(Path(args.before)), read(Path(args.after))
    print(f"before: {len(before)} row(s), {len(before[0]) if before else 0} column(s)")
    print(f"after: {len(after)} row(s), {len(after[0]) if after else 0} column(s)")

    # **The structural roles are not columns of the table**, and that is not a
    # loss: the head, the ground and the projection code go into the
    # `ReferencePointInACrs` and into the CRS, not into a property. They are
    # recognised by the uuid of their property kind - the closed list that
    # `wells-from-table` carries, and never by their name, which belongs to the
    # producer.
    structural_kinds = {
        "348c9a4d-e3e4-4beb-8456-f07a46660de1",  # well name
        "620cec71-5ce3-4ea5-9120-833756b427d7",  # x
        "cfcfdb04-02c1-4262-b66b-459b0aab3e8e",  # y
        "8508cef4-0ade-454a-9ea4-e00d5905e677",  # elevation
        "c48c65d4-9680-4d10-903c-a1b2e30f66b5",  # measured depth
        "d48ef4e7-f82e-4e91-bda2-685146223a85",  # projection code
    }
    structural = set()
    for one in template:
        pid = (one.get("PropertyType") or {}).get("PropertyTypeID", "")
        uuid = pid.split(":")[2] if pid.count(":") >= 2 else ""
        if uuid in structural_kinds and one.get("ColumnName"):
            structural.add(one["ColumnName"])
    roles = {args.well_column, args.md_column} | structural
    shared = [name for name in declared if name not in roles and name in before[0] and name in after[0]]
    missing = [name for name in declared if name not in roles and name in before[0] and name not in after[0]]
    print(f"\ntemplate columns compared: {len(shared)} - {', '.join(shared)}")
    if structural:
        print(f"structural roles, written in the datum and the CRS: {', '.join(sorted(structural))}")
    if missing:
        print(f"  ** missing from the result ({len(missing)}): {', '.join(missing)}")

    # Pairing: (borehole, MD), never the rank.
    keyed: dict[tuple[str, float], dict[str, str]] = {}
    for row in after:
        md = number(row.get("MD", ""))
        if md is None:
            continue
        keyed[(row.get("well", "").strip(), round(md, 6))] = row

    bad: list[str] = []
    unpaired: list[str] = []
    checked = 0
    per_column: dict[str, int] = defaultdict(int)
    for row in before:
        well = (row.get(args.well_column) or "").strip()
        md = number(row.get(args.md_column, ""))
        if md is None:
            continue
        found = keyed.get((well, round(md, 6)))
        if found is None:
            unpaired.append(f"{well} @ {md}")
            continue
        for name in shared:
            checked += 1
            if not same(row.get(name, ""), found.get(name, "")):
                per_column[name] += 1
                bad.append(f"{well} @ {md} - `{name}`: before {row.get(name)!r}, after {found.get(name)!r}")

    print(f"\nrows paired by (borehole, MD): {len(before) - len(unpaired)}/{len(before)}")
    if unpaired:
        print(f"  ** not found ({len(unpaired)}): {', '.join(unpaired[: args.show])}")
    print(f"cells compared: {checked}")
    print(f"cells in disagreement: {len(bad)}")
    for one in bad[: args.show]:
        print(f"  {one}")
    if len(bad) > args.show:
        print(f"  … and {len(bad) - args.show} more")
    if per_column:
        print("  by column: " + ", ".join(f"{k} ({v})" for k, v in sorted(per_column.items())))

    return 0 if not bad and not unpaired and not missing else 1


if __name__ == "__main__":
    sys.exit(main())
