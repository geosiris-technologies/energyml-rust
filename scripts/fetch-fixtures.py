#!/usr/bin/env python3
"""Download the pinned reference packages listed in tests/fixtures.lock.

    python scripts/fetch-fixtures.py              # into target/fixtures/
    python scripts/fetch-fixtures.py --dir DIR    # into DIR
    python scripts/fetch-fixtures.py --update     # print digests, write nothing

Every file is fetched at the exact commit recorded in the lock, its size and
SHA-256 are checked, and any mismatch is a hard failure: a file whose digest
differs is never accepted, and nothing is ever skipped silently.

Needs Python 3.11+ (tomllib) and the standard library only.
"""

import argparse
import hashlib
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    sys.exit("fetch-fixtures: Python 3.11 or newer is required (tomllib).")

ROOT = Path(__file__).resolve().parent.parent
LOCK = ROOT / "tests" / "fixtures.lock"
RAW = "https://raw.githubusercontent.com/{repo}/{commit}/{path}"


def download(url: str) -> bytes:
    """Fetch a URL, retrying a few times on transient network errors."""
    last = None
    for _ in range(3):
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                return response.read()
        except (urllib.error.URLError, TimeoutError) as err:
            last = err
            # An HTTP 404 will not get better by waiting.
            if isinstance(err, urllib.error.HTTPError) and err.code == 404:
                break
    raise RuntimeError(f"cannot download {url}: {last}")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--dir", type=Path, default=ROOT / "target" / "fixtures")
    parser.add_argument("--update", action="store_true",
                        help="print the digest and size of each file; write nothing")
    args = parser.parse_args()

    lock = tomllib.loads(LOCK.read_text(encoding="utf-8"))
    sources = lock["sources"]
    failures = []

    for entry in lock["fixture"]:
        source = sources[entry["source"]]
        dest = args.dir / entry["dest"]
        url = RAW.format(repo=source["repo"], commit=source["commit"], path=entry["path"])

        # A file already in place with the right digest is reused; a wrong one is replaced.
        if not args.update and dest.is_file() and sha256(dest.read_bytes()) == entry["sha256"]:
            print(f"ok      {entry['dest']} (already present)")
            continue

        try:
            data = download(url)
        except RuntimeError as err:
            failures.append(str(err))
            print(f"FAILED  {entry['dest']}: {err}", file=sys.stderr)
            continue

        digest = sha256(data)
        if args.update:
            print(f'{entry["dest"]}: sha256 = "{digest}"  size = {len(data)}')
            continue

        if digest != entry["sha256"] or len(data) != entry["size"]:
            msg = (f"{entry['dest']}: expected sha256 {entry['sha256']} ({entry['size']} bytes), "
                   f"got {digest} ({len(data)} bytes) from {url}")
            failures.append(msg)
            print(f"MISMATCH {msg}", file=sys.stderr)
            continue

        dest.parent.mkdir(parents=True, exist_ok=True)
        # Write beside the target then rename, so a crash never leaves a half file.
        with tempfile.NamedTemporaryFile(dir=dest.parent, delete=False) as tmp:
            tmp.write(data)
        Path(tmp.name).replace(dest)
        print(f"fetched {entry['dest']} ({len(data)} bytes)")

    if failures:
        print(f"\nfetch-fixtures: {len(failures)} failure(s); the fixtures are NOT usable.",
              file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
