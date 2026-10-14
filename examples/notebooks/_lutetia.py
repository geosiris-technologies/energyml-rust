"""Where the notebooks find Lutetia, the reference model they work on.

Lutetia (CC BY 4.0) lives in its own repository,
https://github.com/geosiris-technologies/energyml-lutetia. `lutetia()` answers,
in this order:

1. `$ENERGYML_LUTETIA_DIR`, when it is set: a clone of that repository will do;
2. otherwise the commit this repository pins (`tests/lutetia.lock`), fetched once
   by `scripts/fetch-lutetia.py` into `target/lutetia/` and checked against the
   digests of the lock. A second call finds it there and downloads nothing.

Either way the directory holds `package/` (the model) and `package-with-errors/`
(the same model with deliberate errors).
"""

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def lutetia() -> Path:
    given = os.environ.get("ENERGYML_LUTETIA_DIR")
    if given:
        return Path(given)
    dest = ROOT / "target" / "lutetia"
    subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "fetch-lutetia.py"), "--dest", str(dest)],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    return dest
