#!/usr/bin/env python3
"""Generate `crates/energyml-sheet/src/text/widths.rs` from a Liberation Sans TTF.

Why this script exists: the drilling
sheet wraps text to a *measured* width, in Rust, with no browser and no font
file at run time. That needs a table of horizontal advances, and the table
must come from the font itself - never an invented table (a rule of
this repository). This script reads that table once, by hand, from a real
Liberation Sans file; the Rust crate never opens a font file.

Not part of the crate's build (no `build.rs` reads a system font - that
would make the crate's output depend on whichever font happens to be
installed on whoever's machine compiles it, wasm included, which has no
filesystem to read one from anyway). Run this by hand, once, whenever the
table needs to be regenerated, and commit the *generated* `widths.rs` in
`crates/energyml-sheet/src/text/`. The `.ttf` itself is never copied into
the repository: only the table this script extracts from it.

Usage
-----
    pip install fonttools
    python scripts/sheet-font-widths.py <path-to-LiberationSans-Regular.ttf> \
        [--out crates/energyml-sheet/src/text/widths.rs]
    python scripts/sheet-font-widths.py <path-to-LiberationSans-Regular.ttf> --check \
        [--out crates/energyml-sheet/src/text/widths.rs]

``--check`` regenerates the table in memory and writes nothing. It exits 0 when
the committed file matches, 1 (with a message) when it differs, and 2 when the
font cannot be found - a missing font is never a silent pass. Line endings and
the provenance paragraph (date and font path, which vary by machine and day)
are ignored; the numbers and the code are compared. Give the same font as for
the committed file; the repository carries a copy of Liberation Sans at
``crates/energyml-section/fonts/LiberationSans-Regular.ttf``.

On Windows, Liberation Sans ships with the OS and is usually already at
``C:\\Windows\\Fonts\\LiberationSans-Regular.ttf``. Fedora/RHEL and Debian/
Ubuntu ship the ``liberation-sans-fonts`` / ``fonts-liberation`` package,
typically under ``/usr/share/fonts/``.

License
-------
Liberation Sans is licensed under the SIL Open Font License, Version 1.1
(https://scripts.sil.org/OFL) - verified by this script itself: it reads
the font's own `name` table (nameID 13, "license description") and refuses
to generate anything if that string does not say so. The OFL permits
extracting and redistributing metrics data derived from the font; it does
not need the font's binary to travel with them, which is exactly what this
script buys - the table travels, the font does not.
"""

from __future__ import annotations

import argparse
import datetime
import re
import sys
from pathlib import Path

try:
    from fontTools.ttLib import TTFont
except ImportError:  # pragma: no cover - guidance for whoever runs this by hand
    print("This script needs fontTools: pip install fonttools", file=sys.stderr)
    raise

# The two ranges the crate's `text` module measures directly.
ASCII_START, ASCII_END = 0x20, 0x7E  # space .. tilde
LATIN1_START, LATIN1_END = 0xA0, 0xFF  # NBSP .. y with diaeresis
# 0x7F..0x9F is the C1 control range plus DEL: no glyph, no advance to read.

LICENSE_NAME_ID = 13  # "License Description", per the OpenType `name` table spec


def read_font(path: Path) -> TTFont:
    font = TTFont(str(path))
    license_text = ""
    for record in font["name"].names:
        if record.nameID == LICENSE_NAME_ID:
            try:
                license_text = record.toUnicode()
            except Exception:
                continue
            break
    if "open font license" not in license_text.lower():
        raise SystemExit(
            "refusing to generate a width table: the font at "
            f"{path} does not declare an Open Font License in its own "
            f"`name` table (nameID {LICENSE_NAME_ID}); found: {license_text!r}"
        )
    return font


def advances_per_mille(font: TTFont, start: int, end: int) -> list[int]:
    units_per_em = font["head"].unitsPerEm
    cmap = font.getBestCmap()
    hmtx = font["hmtx"]
    out = []
    missing = []
    for code_point in range(start, end + 1):
        glyph_name = cmap.get(code_point)
        if glyph_name is None:
            missing.append(code_point)
            out.append(0)
            continue
        advance_units, _lsb = hmtx[glyph_name]
        out.append(round(advance_units * 1000 / units_per_em))
    if missing:
        raise SystemExit(
            f"the font has no glyph for code point(s) {missing!r} in "
            f"{start:#x}..{end:#x} - this table must cover the whole range "
            "or the crate's fallback logic would be silently wrong for them"
        )
    return out


def render_table(name: str, start: int, values: list[int]) -> str:
    rows = ",\n".join(
        "    " + ", ".join(str(v) for v in values[i : i + 12])
        for i in range(0, len(values), 12)
    )
    return (
        f"pub(crate) const {name}_START: u32 = {start:#x};\n"
        f"pub(crate) const {name}: [u16; {len(values)}] = [\n{rows},\n];\n"
    )


def mask_provenance(text: str) -> str:
    """Normalise line endings and blank the date/font-path paragraph."""
    text = text.replace("\r\n", "\n")
    return re.sub(
        r"//! Generated by `scripts/sheet-font-widths\.py`.*?\*\*Do not\n",
        "//! <provenance>\n//! **Do not\n",
        text,
        flags=re.S,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("font", type=Path, help="path to LiberationSans-Regular.ttf")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("crates/energyml-sheet/src/text/widths.rs"),
        help="where to write the generated Rust file",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate in memory and exit non-zero if the file at --out differs; writes nothing",
    )
    args = parser.parse_args()

    if not args.font.is_file():
        print(
            f"error: font file not found: {args.font} - the table cannot be "
            "regenerated or checked without Liberation Sans Regular "
            "(see the module docstring for where to find it)",
            file=sys.stderr,
        )
        raise SystemExit(2)

    font = read_font(args.font)
    units_per_em = font["head"].unitsPerEm
    version = ""
    for record in font["name"].names:
        if record.nameID == 5:  # "Version"
            try:
                version = record.toUnicode()
            except Exception:
                pass
            break

    ascii_widths = advances_per_mille(font, ASCII_START, ASCII_END)
    latin1_widths = advances_per_mille(font, LATIN1_START, LATIN1_END)
    default_advance = round(sum(ascii_widths) / len(ascii_widths))

    generated_at = datetime.date.today().isoformat()
    header = f'''//! Horizontal advances of Liberation Sans, in thousandths of the em.
//!
//! Generated by `scripts/sheet-font-widths.py` on {generated_at}, from
//! `{font.reader.file.name if hasattr(font.reader, "file") else args.font}`
//! (`{version}`, `unitsPerEm` {units_per_em}), read with `fontTools`. **Do not
//! edit by hand** - rerun the script against the same font instead.
//!
//! # Why these numbers, and not a guess
//!
//! Sheet design: Liberation Sans is metrically
//! compatible with Arial, so a browser without it substitutes Arial with
//! the *same* widths and every line break computed against this table stays
//! right. The font's own `name` table (nameID 13) states
//! "Licensed under the SIL Open Font License, Version 1.1"
//! (<https://scripts.sil.org/OFL>) - checked by the generator, which refuses
//! to run against a font that does not say so.
//!
//! Two ranges are covered - ASCII (space through tilde) and Latin-1
//! (non-breaking space through y-diaeresis), which is what the measured text
//! of the corpus needs (root's rule: refuse rather than draw, so anything
//! outside these two ranges gets [`DEFAULT_ADVANCE_PER_MILLE`], never one of
//! these numbers pretending to answer for a glyph it was not measured on).
//!
//! [`DEFAULT_ADVANCE_PER_MILLE`] is the arithmetic mean of the ASCII table's
//! own {len(ascii_widths)} entries, rounded to the nearest integer - derived
//! from the same measurement, never a separate invented number.

'''
    body = (
        render_table("ASCII", ASCII_START, ascii_widths)
        + "\n"
        + render_table("LATIN1", LATIN1_START, latin1_widths)
        + "\n"
        + f"/// The mean advance of the ASCII table above, rounded - see the module doc.\n"
        + f"pub(crate) const DEFAULT_ADVANCE_PER_MILLE: u16 = {default_advance};\n\n"
        + '''/// The advance of one character, in thousandths of an em.
///
/// [`DEFAULT_ADVANCE_PER_MILLE`] for anything outside ASCII and Latin-1 -
/// stated, not a silent zero-width character that would make every such
/// glyph free to pack.
pub(crate) fn advance_per_mille(ch: char) -> u16 {
    let code = ch as u32;
    if (ASCII_START..=0x7e).contains(&code) {
        ASCII[(code - ASCII_START) as usize]
    } else if (LATIN1_START..=0xff).contains(&code) {
        LATIN1[(code - LATIN1_START) as usize]
    } else {
        DEFAULT_ADVANCE_PER_MILLE
    }
}
'''
    )
    generated = header + body
    if args.check:
        if not args.out.is_file():
            print(f"error: {args.out} does not exist; run without --check to generate it", file=sys.stderr)
            raise SystemExit(1)
        committed = args.out.read_text(encoding="utf-8")
        if mask_provenance(committed) != mask_provenance(generated):
            print(
                f"error: {args.out} is stale: it differs from the table generated from {args.font}. "
                "Rerun scripts/sheet-font-widths.py without --check and commit the result "
                "(or check that you passed the same font the file was generated from).",
                file=sys.stderr,
            )
            raise SystemExit(1)
        print(f"{args.out} is up to date with {args.font}")
        return
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(generated, encoding="utf-8", newline="\n")
    print(f"wrote {args.out} ({len(ascii_widths)} ASCII + {len(latin1_widths)} Latin-1 advances)")


if __name__ == "__main__":
    main()
