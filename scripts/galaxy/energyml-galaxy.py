#!/usr/bin/env python3
"""Run one `energyml create` operation the way a Galaxy tool calls it.

Written as a proof of concept for the command that goes into the Galaxy tool
XML. It has to handle Galaxy's inputs and outputs, which are always named
`.dat` even when that is not the real extension of the files; the values (the
input and output paths, the different kinds of values...) are passed as
arguments, not through environment variables. Galaxy can also provide
"collections", which are given as a path to a folder that holds the files, so
the CSV reader has to take every CSV of a folder and not just a single one.

**A face, not a library.** Everything an operation decides is decided by the
`energyml` command; this script only does what Galaxy's way of handing files
makes necessary, and nothing the command could be asked instead:

- **a file's kind is given, not read off its name** - Galaxy names every dataset
  `dataset_N.dat`, and the command chooses its reader by the extension. Each
  input says its kind (`--input table:csv=PATH`), and the file is staged under
  that extension, keeping the name Galaxy shows when it is given (`::NAME`,
  `element_identifier` in the tool XML), since an operation reads a stage or a
  date from a file's name;
- **a collection is a folder or a list** - `PATH` may be a directory (every file
  in it, sorted), or several paths joined by commas (a `multiple="true"`
  parameter rendered by Cheetah);
- **a refusal fails the step** - `create` writes a package and exits 0 even when
  arrays were refused;
  any `[refused]` line is an exit code of 1 here;
- **outputs are named by Galaxy** - the package and its HDF5 are copied to the
  paths Galaxy gives, and the URIs of what the step made are written one per line
  (what a downstream surface-modelling tool reads);
- **the run's manifest is the source of the per-role outputs** - `create` is
  called with `--manifest`, the JSON is copied to `--manifest-out`, and each
  `--role-out ROLE=PATH` gets the URIs of that role (every step's, for a
  recipe), one per line. A role file is **always written**: empty when the role wrote
  nothing, and a line of the log says so when the manifest never named it.

Usage (what a tool's `<command>` runs)::

    python3 energyml-galaxy.py run wells-and-properties-from-table \\
        --input 'table:csv=/data/dataset_12.dat::passes.csv' \\
        --template /data/dataset_13.dat \\
        --set project=2154 --set vertical=5720 --set units=STRA \\
        --epc-out /data/dataset_20.dat --h5-out /data/dataset_21.dat \\
        --uris-out /data/dataset_22.dat --log /data/dataset_23.dat

    python3 energyml-galaxy.py run contacts-from-frames \\
        --epc-in /data/dataset_20.dat --h5-in /data/dataset_21.dat \\
        --epc-out … --h5-out … --uris-out … --uris-type PointSetRepresentation
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# The name the arrays of a step are written to, inside the working folder: one
# file, copied to Galaxy's `--h5-out` at the end.
DATA_FILE = "result.h5"

# A line of `create`'s report naming an object it made: `role type uuid title`.
MADE = re.compile(r"^(\S+)\s+(\S+\.\S+)\s+([0-9a-fA-F-]{36})\s*(.*)$")


def paths_of(text: str) -> list[Path]:
    """Every file a `PATH` designates: a directory's files (sorted, hidden ones
    left out), or the comma-separated paths of a multiple input."""
    out: list[Path] = []
    for part in (p.strip() for p in text.split(",")):
        if not part:
            continue
        path = Path(part)
        if path.is_dir():
            out.extend(sorted(child for child in path.iterdir() if child.is_file() and not child.name.startswith(".")))
        else:
            out.append(path)
    return out


def staged_name(source: Path, shown: str | None, extension: str, taken: set[str]) -> str:
    """The name a file is staged under: the name Galaxy shows when given, else
    its own stem; with the kind's extension; unique in the folder."""
    base = Path(shown.strip() if shown and shown.strip() else source.name)
    # `passes.csv` stays; `dataset_12.dat` and `passes` take the kind's extension.
    stem = base.stem if base.suffix.lower() in {".dat", "." + extension} else base.name
    name, n = f"{stem}.{extension}", 1
    while name.lower() in taken:
        n += 1
        name = f"{stem} ({n}).{extension}"
    taken.add(name.lower())
    return name


def stage(source: Path, folder: Path, name: str) -> Path:
    """A copy of `source` under `name` - a copy and not a link, so a container
    that mounts Galaxy's files read-only still works."""
    target = folder / name
    shutil.copyfile(source, target)
    return target


def role_uris(manifest: dict, role: str) -> tuple[list[str], bool]:
    """The URIs of `role` in a run manifest, in plan order, over every
    step; and whether any step named the role at all (written with `count: 0`
    counts: the role is known, it just wrote nothing)."""
    uris: list[str] = []
    named = False
    for step in manifest.get("steps") or []:
        for output in step.get("outputs") or []:
            if output.get("role") == role:
                named = True
                uris.extend(output.get("uris") or [])
    return uris, named


def write_roles(manifest: dict | None, roles: list[str], log: list[str]) -> int:
    """One file per `ROLE=PATH`. Never a missing file: empty when there is
    nothing, and a line in `log` when the role is not in the manifest."""
    for given in roles:
        role, _, path = given.partition("=")
        if not role or not path:
            print(f"--role-out `{given}`: expected ROLE=PATH", file=sys.stderr)
            return 2
        uris, named = role_uris(manifest, role) if manifest is not None else ([], False)
        if not named:
            log.append(f"role `{role}` is not in the manifest: its output is empty")
        Path(path).write_text("".join(uri + "\n" for uri in uris), encoding="utf-8")
    return 0



def run(args: argparse.Namespace) -> int:
    energyml = args.energyml
    with tempfile.TemporaryDirectory(prefix="energyml-galaxy-") as tmp:
        work = Path(tmp)
        inputs, out = work / "in", work / "out"
        inputs.mkdir()
        out.mkdir()
        taken: set[str] = set()
        command = [energyml, "create", args.operation]

        # Inputs: `SLOT:KIND=PATH[::NAME]`. Every file of a folder or a list goes
        # to the same slot, which `create` numbers (`table`, `table 2`, …).
        for given in args.input:
            head, _, rest = given.partition("=")
            slot, _, kind = head.partition(":")
            if not rest or not kind:
                print(f"--input `{given}`: expected SLOT:KIND=PATH[::NAME]", file=sys.stderr)
                return 2
            where, _, shown = rest.partition("::")
            files = paths_of(where)
            if not files:
                print(f"--input `{given}`: no file there", file=sys.stderr)
                return 2
            for at, path in enumerate(files):
                # A name shown by Galaxy is one file's; a folder's files keep their own.
                name = staged_name(path, shown if len(files) == 1 else None, kind, taken)
                command += ["--from", f"{slot}={stage(path, inputs, name)}"]

        # Objects of the package for a slot: `SLOT=URI[,URI…]` or `SLOT=@FILE`
        # (one per line or comma-separated, what an upstream `--uris-out` wrote).
        for given in args.object:
            slot, _, rest = given.partition("=")
            listed = Path(rest[1:]).read_text(encoding="utf-8") if rest.startswith("@") else rest
            for uri in (u.strip() for u in re.split(r"[,\n]", listed)):
                if uri:
                    command += ["--from", f"{slot}={inputs / 'input.epc'}#{uri}"]

        if args.template:
            command += ["--template", str(stage(Path(args.template), inputs, "template.json"))]
        for choice in args.set:
            # An optional parameter left unset reaches Galaxy's command as `None`
            # (a workflow saved before the parameter existed): it is no choice,
            # and the operation's own default applies. Tools generated before
            # 07/10/2026 let it through their guard.
            name, _, value = choice.partition("=")
            if value.strip() in ("", "None"):
                print(f"`{name}` left unset (Galaxy gave {value.strip() or 'nothing'!r}): the operation's default applies", flush=True)
                continue
            command += ["--set", choice]

        # The package to add to, and its arrays: read from the input, written to
        # a copy - `create` cannot write into a file it is reading (05/10).
        if args.epc_in:
            command += ["--into", str(stage(Path(args.epc_in), inputs, "input.epc"))]
            if args.h5_in:
                command += ["--h5", str(stage(Path(args.h5_in), inputs, "input.h5"))]
                shutil.copyfile(args.h5_in, out / DATA_FILE)
        elif args.object:
            print("--object names objects of a package: give it with --epc-in", file=sys.stderr)
            return 2
        command += ["--set", f"dataFile={DATA_FILE}", "--out", str(out / "result.epc")]
        manifest_file = out / "manifest.json"
        if args.manifest_out or args.role_out:
            command += ["--manifest", str(manifest_file)]

        done = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=out)
        report = " ".join(command) + "\n\n" + done.stdout + done.stderr
        if args.log:
            Path(args.log).write_text(report, encoding="utf-8")
        sys.stdout.write(done.stdout)
        sys.stderr.write(done.stderr)

        refused = [line for line in done.stdout.splitlines() if line.lstrip().startswith("[refused]")]
        if done.returncode != 0:
            return done.returncode
        if refused:
            print(f"{len(refused)} refusal(s) - the step is not a result", file=sys.stderr)
            return 1

        shutil.copyfile(out / "result.epc", args.epc_out)
        if args.h5_out:
            if not (out / DATA_FILE).exists():
                print(f"no {DATA_FILE} was written: the operation stored no array", file=sys.stderr)
                return 1
            shutil.copyfile(out / DATA_FILE, args.h5_out)
        if args.uris_out:
            made = [MADE.match(line) for line in done.stdout.splitlines()]
            uris = [
                f"eml:///{m.group(2)}({m.group(3).lower()})"
                for m in made
                if m and (not args.uris_type or m.group(2).split(".")[-1] == args.uris_type)
            ]
            Path(args.uris_out).write_text("".join(uri + "\n" for uri in uris), encoding="utf-8")

        # The manifest, and the file of each role: always written, even empty.
        manifest = None
        if manifest_file.exists():
            manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
            if args.manifest_out:
                shutil.copyfile(manifest_file, args.manifest_out)
        elif args.manifest_out or args.role_out:
            print("`energyml create` wrote no manifest", file=sys.stderr)
            if args.manifest_out:
                Path(args.manifest_out).write_text("{}\n", encoding="utf-8")
        notes: list[str] = []
        status = write_roles(manifest, args.role_out, notes)
        for note in notes:
            print(note)
        if args.log and notes:
            with open(args.log, "a", encoding="utf-8") as log:
                log.write("\n" + "\n".join(notes) + "\n")
        if status:
            return status
    return 0


def self_test() -> int:
    """No Galaxy, no energyml: the role files, from a run manifest."""
    manifest = {
        "format": "energyml-run-outputs/1",
        "package": {"epc": "out.epc", "dataFiles": ["result.h5"]},
        "recipe": "r",
        "steps": [
            {"name": "a", "operation": "x", "outputs": [
                {"role": "representation", "declared": True, "count": 2, "uris": ["eml:///a.A(1)", "eml:///a.A(2)"]},
                {"role": "frame", "declared": True, "count": 0, "uris": []},
            ], "refused": 0},
            {"name": "b", "operation": "y", "outputs": [
                {"role": "representation", "declared": True, "count": 1, "uris": ["eml:///a.A(3)"]},
            ], "refused": 0},
        ],
    }
    with tempfile.TemporaryDirectory() as tmp:
        files = {role: Path(tmp) / role for role in ("representation", "frame", "ghost")}
        log: list[str] = []
        status = write_roles(manifest, [f"{r}={p}" for r, p in files.items()], log)
        assert status == 0, status
        assert files["representation"].read_text() == "eml:///a.A(1)\neml:///a.A(2)\neml:///a.A(3)\n"
        assert files["frame"].read_text() == "", "a declared role that wrote nothing: an empty file"
        assert files["ghost"].exists() and files["ghost"].read_text() == "", "an unknown role: still a file"
        assert log == ["role `ghost` is not in the manifest: its output is empty"], log
        assert "frame" not in "".join(log), "a role named with count 0 is known"
        # No manifest at all: every file empty.
        write_roles(None, [f"frame={files['frame']}"], [])
        assert files["frame"].read_text() == ""
        for role, path in files.items():
            print(f"{role}: {path.read_text()!r}")
    print("self-test ok")
    return 0


def main() -> int:
    # Galaxy's logs and a Windows console may not be UTF-8: the command's report
    # names titles with accents and arrows, and printing must not be what fails.
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    if "--self-test" in sys.argv[1:]:
        return self_test()
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="verb", required=True)
    one = sub.add_parser("run", help="run one operation, Galaxy's files in and out")
    one.add_argument("operation", help="the operation, as `energyml create --list` names it")
    one.add_argument("--input", action="append", default=[], metavar="SLOT:KIND=PATH[::NAME]",
                     help="a file for a slot, of a kind (csv, geojson, obj…) - PATH may be a folder or a comma list")
    one.add_argument("--object", action="append", default=[], metavar="SLOT=URI|@FILE",
                     help="objects of the input package for a slot")
    one.add_argument("--template", help="what the tables' columns mean (an OSDU template, JSON)")
    one.add_argument("--set", action="append", default=[], metavar="NAME=VALUE", help="a choice of the operation")
    one.add_argument("--epc-in", help="the package to add to")
    one.add_argument("--h5-in", help="its arrays")
    one.add_argument("--epc-out", required=True, help="where the package goes")
    one.add_argument("--h5-out", help="where its arrays go")
    one.add_argument("--uris-out", help="the URIs of the objects made, one per line")
    one.add_argument("--uris-type", help="only those of this type (`PointSetRepresentation`)")
    one.add_argument("--manifest-out", help="where the run's manifest (JSON) goes")
    one.add_argument("--role-out", action="append", default=[], metavar="ROLE=PATH",
                     help="where the URIs of a role go, one per line (empty when it wrote nothing)")
    one.add_argument("--log", help="the command and its report")
    one.add_argument("--energyml", default=os.environ.get("ENERGYML", "energyml"), help="the energyml binary")
    args = parser.parse_args()
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
