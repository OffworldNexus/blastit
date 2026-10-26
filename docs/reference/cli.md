# Command line

Configuration is the environment; `--preset` is the only argument an operator
needs.

## `blastit`

```console
$ blastit
$ blastit --preset digitalocean-paas
$ blastit --help
```

| Argument | What it does |
|---|---|
| *(none)* | runs the server, configuration from the environment |
| `--preset NAME`, `--preset=NAME` | one platform's defaults; a set `BLASTIT_*` variable (or `PORT`) still wins |
| `--help`, `-h` | prints the usage and the preset list |

`--preset` is a stage 0 argument; `--stage=1` and `--role=…` refuse it. There is
no `--version`; the boot log's [`preload` event](log-events.md#the-three-boot-lines)
identifies the build.

`blastit` is the wheel's console script, so `uv run blastit` uses your project's
environment. It re-executes that interpreter for stage 1, making your packages
importable in every view.

## Internal arguments

Not for operators; a mistake exits `1`.

| Argument | Role |
|---|---|
| `--stage=1 --config-fd=N [--dbproxy-pid=P --dbproxy-events-fd=N --dbproxy-stdout-fd=N --dbproxy-stderr-fd=N]` | stage 1: the application environment, after the `execve` barrier |
| `--role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0\|1]` | the database proxy, `execve`'d with an empty environment |
| `--seccomp=0\|1` | `BLASTIT_SECCOMP`, for the one role that cannot read the environment |

`--role=dbproxy` grants nothing by itself: the credentials arrive over the pipe
the flag names. A wheel install refuses it, since the process holding them must
run no Python. http, the scheduler and a view have no flag; their place in the
process tree chooses them.

## `blastit-manifest fold`

Reads blastit's JSON events and the manifest, and says what grants are missing.

```console
$ blastit-manifest fold LOG... --manifest blastit.toml [--write] [--prefer-tables] [--allow-dangerous]
```

| Argument | What it does |
|---|---|
| `LOG`… | the logs to read; `-` reads standard input |
| `--manifest PATH` (required) | the `blastit.toml` to extend |
| `--write` | rewrite the manifest in place, not print it |
| `--prefer-tables` | add the smallest `tables` grant covering the denials |
| `--allow-dangerous` | grant statements a grant is not an allowlist for |

Exits `0` on a missing-grant diff, empty or not, `1` on an unreadable log or manifest.
