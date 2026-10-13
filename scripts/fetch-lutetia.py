#!/usr/bin/env python3
"""Fetch the pinned Lutetia reference model into the directory the tests read.

    python scripts/fetch-lutetia.py --dest target/lutetia

Reads tests/lutetia.lock (repository and exact commit of `energyml-lutetia`),
downloads that commit's archive from GitHub, and lays out the directory that
`$ENERGYML_LUTETIA_DIR` points to:

    <dest>/package/Lutetia.epc (+ Lutetia.h5)           the clean model
    <dest>/package-with-errors/Lutetia.epc (+ .h5)      the variant with deliberate errors
    <dest>/recipe/recette.toml, recipe/donnees/, recipe/energyml-version
    <dest>/written-on                                   the platform stamp of the replay, when present

The contract is described in the README of `energyml-lutetia`. Every file listed
under `[sha256]` in the lock is checked, and any mismatch is a hard failure.
Nothing is skipped silently.

When it succeeds, the last line printed is `ENERGYML_LUTETIA_DIR=<absolute path>`
so that a CI step can export it. Exit 0 on success; non-zero, with a message on
stderr, on any failure (a placeholder commit in the lock included).

Needs Python 3.11+ (tomllib) and the standard library only.
"""

import argparse
import hashlib
import shutil
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    sys.exit("fetch-lutetia: Python 3.11 or newer is required (tomllib).")

ROOT = Path(__file__).resolve().parent.parent
LOCK = ROOT / "tests" / "lutetia.lock"
ARCHIVE = "https://codeload.github.com/{repo}/tar.gz/{commit}"
MARKER = ".fetched-commit"

# What the tests read, relative to the repository root. Anything else (docs,
# scripts, licences) is left in the archive.
KEPT_TREES = ("package", "package-with-errors", "recipe")
KEPT_FILES = ("written-on",)
# Without these the directory is not the contract's.
REQUIRED = (
    "package/Lutetia.epc",
    "package/Lutetia.h5",
    "recipe/recette.toml",
)
REQUIRED_ONE_OF = (
    ("package-with-errors/Lutetia-with-errors.epc", "package-with-errors/Lutetia.epc"),
)


def fail(message: str):
    sys.exit(f"fetch-lutetia: {message}")


def download(url: str, target: Path) -> None:
    """Fetch a URL into a file, retrying a few times on transient network errors."""
    last = None
    for _ in range(3):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "energyml-fetch-lutetia"})
            with urllib.request.urlopen(request, timeout=300) as response, target.open("wb") as out:
                shutil.copyfileobj(response, out)
            return
        except (urllib.error.URLError, TimeoutError) as err:
            last = err
            # An HTTP 404 will not get better by waiting.
            if isinstance(err, urllib.error.HTTPError) and err.code == 404:
                break
    fail(f"cannot download {url}: {last}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def wanted(relative: str) -> bool:
    return relative in KEPT_FILES or relative.split("/", 1)[0] in KEPT_TREES


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--dest", type=Path, required=True, help="the directory to lay out (ENERGYML_LUTETIA_DIR)")
    args = parser.parse_args()

    if not LOCK.is_file():
        fail(f"{LOCK} not found")
    lock = tomllib.loads(LOCK.read_text(encoding="utf-8"))
    source = lock.get("source", {})
    repo, commit = source.get("repo"), source.get("commit", "")
    if not repo or not commit:
        fail(f"{LOCK} must give [source] repo and commit")
    if len(commit) != 40 or set(commit) <= {"0"} or any(c not in "0123456789abcdef" for c in commit):
        fail(
            f"{LOCK} holds a placeholder commit ({commit!r}): pin a real 40-hex commit of {repo} "
            "(the energyml-lutetia repository must be published first)"
        )

    dest = args.dest.resolve()
    if (dest / MARKER).is_file() and (dest / MARKER).read_text(encoding="utf-8").strip() == commit:
        print(f"already present: {dest} at {commit}")
    else:
        url = ARCHIVE.format(repo=repo, commit=commit)
        with tempfile.TemporaryDirectory() as scratch:
            archive = Path(scratch) / "lutetia.tar.gz"
            print(f"fetching {url}", flush=True)
            download(url, archive)
            staged = Path(scratch) / "tree"
            staged.mkdir()
            with tarfile.open(archive, "r:gz") as tar:
                for member in tar.getmembers():
                    # The archive has one top folder, `<repo name>-<commit>/`.
                    parts = member.name.split("/", 1)
                    relative = parts[1] if len(parts) == 2 else ""
                    if not relative or not wanted(relative):
                        continue
                    if member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                        fail(f"refusing the archive member {member.name!r}: only plain files and folders are laid out")
                    if relative.startswith("/") or ".." in relative.split("/"):
                        fail(f"refusing the archive member {member.name!r}: it leaves the destination")
                    member.name = relative
                    # `filter` exists from Python 3.12 (and in late 3.11 patch releases).
                    try:
                        tar.extract(member, staged, filter="data")
                    except TypeError:
                        tar.extract(member, staged)

            for relative, digest in lock.get("sha256", {}).items():
                path = staged / relative
                if not path.is_file():
                    fail(f"{relative} is in the lock but not in {repo}@{commit}")
                got = sha256(path)
                if got != digest:
                    fail(f"{relative}: expected sha256 {digest}, got {got}")

            missing = [name for name in REQUIRED if not (staged / name).is_file()]
            missing += [" or ".join(names) for names in REQUIRED_ONE_OF if not any((staged / n).is_file() for n in names)]
            if missing:
                fail(f"{repo}@{commit} does not lay out the contract; missing: {', '.join(missing)}")

            # The archive is whole and checked: only now is the destination touched.
            dest.mkdir(parents=True, exist_ok=True)
            for name in (*KEPT_TREES, *KEPT_FILES):
                target = dest / name
                if target.is_dir():
                    shutil.rmtree(target)
                elif target.exists():
                    target.unlink()
                if (staged / name).exists():
                    shutil.move(str(staged / name), str(target))
            (dest / MARKER).write_text(commit + "\n", encoding="utf-8")
        print(f"laid out {dest} from {repo}@{commit}")

    print(f"ENERGYML_LUTETIA_DIR={dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
