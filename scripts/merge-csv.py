#!/usr/bin/env python3
"""Paste several CSV files into one, with **one** header.

Written for the tables work: the BRGM corpus is one file per borehole (120 of
them) and the same operation must be tryable on the whole batch.

    python scripts/merge-csv.py <folder-or-files...> -o all.csv

# What this script refuses to do

- **paste different headers.** Two files whose columns are not the same, in the
  same order, give a CSV in which half the rows are shifted - perfectly
  well-formed text that nothing downstream can detect. It stops, naming the
  first file that differs **and** what differs: a column too many, one too few,
  an order changed;
- **guess the separator line by line.** It is decided once, on the header of the
  first file, outside quotes - the rule of the Rust reader
  (`crates/energyml-build/src/tabular.rs`), so that this script and the reader
  never read the same file differently;
- **rewrite cells.** Rows are copied **as they are**, quotes included.
  Reformatting them would pass them through a decode and a re-encode, hence
  through our conventions rather than the producer's - and the corpus carries
  `"48° 54' 50.51""N, 2° 24' 15.30""E"`, which an approximate round trip would
  break silently.

# What it says

The number of files, of rows, and the files **empty of data** - a file that has
only a header is news, not a detail: it will produce no borehole, and someone
will look for it in the package.
"""

from __future__ import annotations

import argparse
import csv
import io
import sys
from pathlib import Path


def sniff(line: str) -> str:
    """The separator, decided **once** and outside quotes.

    The same rule as `Dialect::sniff`: counting inside a quoted field would turn
    a line with three commas inside one field into a comma-separated file.
    """
    best, most = ",", 0
    for candidate in (";", ",", "\t"):
        quoted, count = False, 0
        for c in line:
            if c == '"':
                quoted = not quoted
            elif c == candidate and not quoted:
                count += 1
        if count > most:
            best, most = candidate, count
    return best


def header_of(text: str, separator: str) -> list[str]:
    """The column names, read with a real CSV reader."""
    rows = csv.reader(io.StringIO(text), delimiter=separator)
    for row in rows:
        return row
    return []


def differs(reference: list[str], other: list[str]) -> str | None:
    """What separates two headers, in one sentence - or `None`."""
    if reference == other:
        return None
    if len(reference) != len(other):
        return f"{len(other)} column(s) against {len(reference)}"
    missing = [name for name in reference if name not in other]
    extra = [name for name in other if name not in reference]
    if missing or extra:
        return f"missing: {missing or '-'}; extra: {extra or '-'}"
    return "the same columns, in another order"


def read(path: Path) -> str:
    """The text, as UTF-8 then as cp1252 - Windows exports are common."""
    raw = path.read_bytes()
    for encoding in ("utf-8-sig", "utf-8", "cp1252"):
        try:
            return raw.decode(encoding)
        except UnicodeDecodeError:
            continue
    raise SystemExit(f"{path}: neither UTF-8 nor cp1252")


def main() -> int:
    # **The Windows console is not UTF-8**, and this script prints accented file
    # names: without this line it dies on its own report, after having written
    # the file - the worst moment, since everything worked and the trace says
    # otherwise.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")

    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sources", nargs="+", help=".csv files, or a folder that holds some")
    parser.add_argument("-o", "--out", required=True, help="the file to write")
    parser.add_argument(
        "--allow-different-headers",
        action="store_true",
        help="paste anyway - the rows will be shifted, and it says so",
    )
    args = parser.parse_args()

    files: list[Path] = []
    for source in args.sources:
        at = Path(source)
        if at.is_dir():
            files.extend(sorted(at.glob("*.csv")))
        elif at.is_file():
            files.append(at)
        else:
            raise SystemExit(f"{at}: neither a file nor a folder")
    if not files:
        raise SystemExit("no .csv file")

    out = Path(args.out)
    first = read(files[0])
    separator = sniff(first.splitlines()[0] if first.splitlines() else "")
    reference = header_of(first, separator)
    if not reference:
        raise SystemExit(f"{files[0]}: no header")

    total, empty, refused = 0, [], []
    with out.open("w", encoding="utf-8", newline="") as sink:
        sink.write(first.splitlines(keepends=True)[0] if first.splitlines() else "")
        for path in files:
            text = read(path) if path != files[0] else first
            lines = text.splitlines(keepends=True)
            if not lines:
                empty.append(path.name)
                continue
            head = header_of(text, separator)
            said = differs(reference, head)
            if said:
                message = f"{path.name}: {said}"
                if not args.allow_different_headers:
                    raise SystemExit(
                        f"{message}\n"
                        "Pasting different headers shifts half the rows without anything saying so.\n"
                        "Use --allow-different-headers if that is really wanted."
                    )
                refused.append(message)
            body = lines[1:]
            if not any(one.strip() for one in body):
                empty.append(path.name)
                continue
            for one in body:
                if not one.strip():
                    continue
                # **As is**: neither decoded nor re-encoded. The corpus carries
                # quoted fields containing commas and doubled quotes, which an
                # approximate round trip would break.
                sink.write(one if one.endswith("\n") else one + "\n")
                total += 1

    print(f"{len(files)} file(s) → {out}")
    print(f"  separator: {separator!r}   columns: {len(reference)}   data rows: {total}")
    if empty:
        print(f"  with no data row at all ({len(empty)}): {', '.join(empty[:8])}{' …' if len(empty) > 8 else ''}")
    for one in refused:
        print(f"  [different header] {one}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
