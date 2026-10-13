# Galaxy tools

Every operation of `energyml create` can be offered to a
[Galaxy](https://galaxyproject.org) server as a tool. **The tool is never written
by hand**: `energyml galaxy-tool` renders its XML from the operation's
declaration (`energyml_build::galaxy`), and the tool runs the public `energyml`
image. An operation added to the registry has its tool without a line written
for it.

```text
Galaxy -- docker run <image> /bin/sh -c "<command>" --> /opt/energyml/scripts/galaxy/energyml-galaxy.py
                                                              | (in the image)
                                                              v
                                                      energyml create <op> ...
```

## Generating a tool

`galaxy-tool` is a **hidden** subcommand: it does not appear in `energyml --help`
or in the generated command reference.

```sh
energyml galaxy-tool contacts-from-frames --out out/galaxy-tools/energyml_contacts_from_frames.xml
energyml galaxy-tool contacts-from-frames            # the XML on the standard output
energyml galaxy-tool --all --dir out/galaxy-tools    # one file per operation of the registry
```

| Option | Meaning |
|---|---|
| `<operation>` | the operation, as `energyml create --list` names it |
| `--out FILE.xml`, `-o` | where to write; standard output when absent |
| `--all --dir DIR` | every operation, `energyml_<operation>.xml` each (`-` becomes `_`); `--all` requires `--dir` |
| `--image IMAGE` | the container the tool runs in. Default: `ghcr.io/geosiris-technologies/energyml:latest`. Point it at your own image to use a private build |
| `--runner PATH` | the runner's path inside the image (default `/opt/energyml/scripts/galaxy/energyml-galaxy.py`) |

**Regenerate rather than edit**: each file says so in a comment. The tool's
`version` is the version of the crate that wrote it. A tool and its image must go
together, because the XML is written for the arguments of the runner of the image
it names.

## What the XML holds

### Inputs

What an operation **takes** becomes Galaxy `data` inputs:

| The operation takes | The tool offers |
|---|---|
| tables | a `csv` input (`multiple` when the slot takes several) and a required `template` input (`json`) saying what the columns mean |
| features | a `json` input (read as GeoJSON) |
| a document | a `data` input |
| objects of the package | two optional inputs: URIs typed in (`text`, comma-separated) and a file of URIs - what an upstream step's URI output holds |
| anything else (a ramp, a lattice) | nothing: these are pieces a page builds, not files a workflow passes; the tool's help lists them under *Not offered here* |

Beside them, `epc_in` and `h5_in` (the package to add to and its arrays) are
always there. The package is required only when a slot must be given objects of
it; otherwise the operation writes a new package.

What an operation **asks** becomes parameters: a number is a `float` (with `min`,
`max`, the unit in the label), an integer an `integer`, an EPSG code an `integer`
with `min=1`, a flag a `boolean`, a closed choice a `select`, text and
vocabulary terms `text`.

An optional parameter left unset reaches the command as `None`; every guard in
the generated command refuses both `''` and `'None'`, so the operation's own
default applies. A test refuses any tool whose guard does not.

### Outputs

| Output | Format | What it holds |
|---|---|---|
| `epc_out` | `epc` | the package |
| `h5_out` | `h5` | its arrays |
| `uris_out` | `txt` | URIs of the objects made, one per line (filtered by `uris_type`) |
| `manifest` | `json` | what the run wrote, by role |
| `uris_<role>` | `txt` | one per role the operation declares: the URIs of that role, empty when the run wrote nothing under it |
| `log` | `txt` | the command and its report |

A downstream tool takes `uris_<role>` as its *file of URIs*: that is how the
steps of a workflow chain. An operation whose declared outputs are empty is *not
declared yet*, not "writes nothing".

## The runner

Galaxy names every dataset `dataset_N.dat`, whatever it holds, while `energyml`
chooses a reader by extension. The runner `energyml-galaxy.py` (in the image at `/opt/energyml/scripts/galaxy/`, in the repository at `scripts/galaxy/`) does
what that requires and nothing the command could be asked instead; every
decision about an operation stays in `energyml create`.

```sh
python3 energyml-galaxy.py run wells-and-properties-from-table \
    --input 'table:csv=/data/dataset_12.dat::passes.csv' \
    --template /data/dataset_13.dat \
    --set project=2154 --set units=STRA \
    --epc-out /data/dataset_20.dat --h5-out /data/dataset_21.dat \
    --uris-out /data/dataset_22.dat --log /data/dataset_23.dat
```

| Argument | Meaning |
|---|---|
| `--input SLOT:KIND=PATH[::NAME]` | a file for a slot, staged under the extension its kind says; `PATH` may be a folder or comma-joined paths; `::NAME` is the name Galaxy shows (kept because an operation may read a stage or date from a file name). Repeatable |
| `--object SLOT=URI[,URI...]` or `SLOT=@FILE` | objects of the input package for a slot; needs `--epc-in` |
| `--template FILE` | what the tables' columns mean (JSON) |
| `--set NAME=VALUE` | a choice of the operation; ignored when the value is empty or `None` |
| `--epc-in`, `--h5-in` | the package to add to and its arrays (staged as copies) |
| `--epc-out`, `--h5-out` | where the package and its arrays go |
| `--uris-out`, `--uris-type` | URIs of the objects made |
| `--manifest-out`, `--role-out ROLE=PATH` | the run manifest, and one file of URIs per role |
| `--log` | the command and its report |
| `--energyml` | the binary to call (default `$ENERGYML`, else `energyml`) |

It **fails the step on any `[refused]` line** of the report: `create` writes a
package and exits 0 even when arrays were refused, so a refusal is exit code 1
here. A role file is always written, empty when the role wrote nothing.
`python3 energyml-galaxy.py --self-test` checks the role files against a manifest
without Galaxy and without `energyml`.

## Installing the tools in a Galaxy server

1. Generate the XML (`energyml galaxy-tool --all --dir out/galaxy-tools`).
2. Copy the files into your server's tools folder, in a sub-folder of their own.
3. Declare them in `tool_conf.xml`, in a section of their own:

   ```xml
   <section id="energyml" name="EnergyML">
       <tool file="energyml/energyml_contacts_from_frames.xml" />
       <!-- one line per file -->
   </section>
   ```

4. Pull the image on the machines that run jobs and allow Galaxy to run Docker
   containers for its tools.
5. Restart Galaxy or reload the tool configuration.

TODO(verify): publication of the tools on the Galaxy ToolShed.

## The run manifest

`energyml create --manifest FILE.json` writes, and the tool's `manifest` output
holds, what a run wrote - from what `apply` wrote, not from the plan:

```json
{
  "format": "energyml-run-outputs/1",
  "package": { "epc": "out.epc", "dataFiles": ["result.h5"] },
  "steps": [
    { "name": "contacts-from-frames", "operation": "contacts-from-frames",
      "outputs": [ { "role": "representation", "declared": true, "count": 8,
                     "types": ["resqml22.PointSetRepresentation"], "uris": ["eml:///..."] } ],
      "refused": 0 }
  ]
}
```

## Describing the operations

```sh
energyml create --list --json                         # every operation
energyml create --list --json contacts-from-frames    # one
```

Nothing but JSON goes to standard output and no input is read. Each operation
carries `name`, `describe`, `makes`, `category`, `needs`, `takes`, `reports` and
`gives`. It is the description the Galaxy tools are written from, and what any
orchestrator reads to know what an operation takes and returns.

## Compatibility

The runner's arguments are a contract with every workflow engine that calls it.
Changing what they mean breaks those callers silently: treat them as a public
interface and version them.
