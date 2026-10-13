# Logging

The operations and the package writer emit `tracing` spans and events.
**Nothing is shown by default**: with no filter asked, no subscriber is
installed and every command prints exactly what it printed before.

A span reports how long it took (`time.busy`, `time.idle` on its closing line),
so the log answers "which step is slow" without a profiler. A log is not a
report: what you must know about a run (refusals, notes, what was set aside)
stays in the operation's report; the log says where the time went.

## From the command line

```sh
energyml create --recipe recipe.toml --out Lutetia.epc --log info
energyml create ... --log energyml_build=debug,energyml_storage=info
ENERGYML_LOG=info energyml create ...         # the same, from the environment
energyml create ... --log info --log-format json 2> journal.jsonl
```

- `--log <filter>` (or `ENERGYML_LOG`) is a `tracing_subscriber` `EnvFilter`: a
  level (`error`, `warn`, `info`, `debug`, `trace`, `off`) or a comma-separated
  list of `target=level`. Targets are crate names with underscores:
  `energyml_build`, `energyml_storage`.
- `--log-format text|json` (or `ENERGYML_LOG_FORMAT`): human lines or one JSON
  object per line. The flag wins over the variable.
- **Always stderr**, never stdout: what a script reads from stdout is untouched.
- `info` shows one line per span close (operation, recipe step, read, write) with
  its elapsed time; `debug` adds per-object detail. There is never an event per
  array element.

## In Docker

```sh
docker run --rm -e ENERGYML_LOG=info -v "$PWD:/work" ghcr.io/geosiris-technologies/energyml create --recipe recipe.toml --out out.epc
docker run --rm -e ENERGYML_LOG=info -e ENERGYML_LOG_FORMAT=json -v "$PWD:/work" ghcr.io/geosiris-technologies/energyml ... 2> journal.jsonl
```

## Changing the level while a program runs

`energyml-log` is built for an application that is already running:

```rust
// At start: install the subscriber, logs off.
let handle = energyml_log::init_reloadable(None, None)?;
// Later, from a menu:
handle.set_filter("debug")?;                 // on
handle.set_filter("energyml_build=trace")?;  // one crate
handle.set_filter("off")?;                   // and off again
```

`init` installs nothing when no filter is given (a command-line tool);
`init_reloadable` always installs, starting off, so the level can be raised
later. An invalid filter is refused and leaves the current one in force.
`energyml_log::dispatch` builds the same subscriber over any writer without
installing it, which is how tests check the switch.

## For library authors

Libraries depend on `tracing` only and never install anything. Spans on phases,
counts as fields, no event per element.
