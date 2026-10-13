#!/usr/bin/env python3
"""Build the documentation of every published crate the way docs.rs does.

    python scripts/docsrs-check.py                 # every publishable crate
    python scripts/docsrs-check.py energyml-core   # one crate, or several

docs.rs compiles a crate on a nightly toolchain, with `--cfg docsrs` given to
rustdoc, and with the features its `[package.metadata.docs.rs]` asks for
(`all-features`, `features`, `no-default-features`). A crate that builds with
`cargo doc` on stable can still fail there: a feature that needs a native
library the docs.rs builder does not have, an unstable attribute guarded by
`cfg(docsrs)` that does not compile, a broken intra-doc link under a feature.
This script does the same, so it is found before a release rather than after.

Needs `cargo +nightly`. Exit status 0 when every crate builds, 1 otherwise.
Standard library only.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys


def main() -> int:
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True, capture_output=True, text=True).stdout)
    wanted = set(sys.argv[1:])
    env = dict(os.environ)
    env["RUSTDOCFLAGS"] = (env.get("RUSTDOCFLAGS", "") + " --cfg docsrs").strip()
    failed: list[str] = []
    built = 0
    for pkg in sorted(meta["packages"], key=lambda p: p["name"]):
        name = pkg["name"]
        if wanted and name not in wanted:
            continue
        if pkg.get("publish") == []:  # `publish = false`
            continue
        docs = (pkg.get("metadata") or {}).get("docs", {}).get("rs", {})
        cmd = ["cargo", "+nightly", "doc", "--no-deps", "-p", name]
        if docs.get("all-features"):
            cmd.append("--all-features")
        if docs.get("no-default-features"):
            cmd.append("--no-default-features")
        if docs.get("features"):
            cmd += ["--features", ",".join(docs["features"])]
        cmd += docs.get("cargo-args", [])
        print(f"::group::docs.rs {name}: {' '.join(cmd[2:])}", flush=True)
        built += 1
        rc = subprocess.run(cmd, env=env).returncode
        print("::endgroup::", flush=True)
        if rc != 0:
            print(f"::error::{name} does not build its documentation as docs.rs does")
            failed.append(name)
    if wanted and built != len(wanted):
        print(f"error: asked for {sorted(wanted)}, built {built}", file=sys.stderr)
        return 2
    if built == 0:
        print("error: no publishable crate found; the check would prove nothing", file=sys.stderr)
        return 2
    print(f"{built} crates built, {len(failed)} failed" + (": " + ", ".join(failed) if failed else ""))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
