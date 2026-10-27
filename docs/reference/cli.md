# Command line

Configuration is the environment; `--preset` is the only argument you need.

## `blastit`

```console
$ blastit
$ blastit --preset digitalocean-paas
$ blastit --help
```

| Argument | What it does |
|---|---|
| *(none)* | runs the server, configuration from the environment |
| `--preset NAME`, `--preset=NAME` | one platform's defaults; a set `BLASTIT_*` or `PORT` still wins |
| `--help`, `-h` | prints the usage and the preset list |

`--preset` belongs to stage 0: `--stage=1` and `--role=…` refuse it. No
`--version`: the [`preload` event](log-events.md#the-three-boot-lines) names the
build.

`blastit` is the wheel's console script: `uv run blastit` uses your project's
environment, and stage 1 re-executes that interpreter, so your packages are
importable in every view.

## Internal arguments

Not for operators; a mistake exits `1`.

| Argument | Role |
|---|---|
| `--stage=1 --config-fd=N [--dbproxy-pid=P --dbproxy-events-fd=N --dbproxy-stdout-fd=N --dbproxy-stderr-fd=N]` | stage 1: the application environment, after the `execve` barrier |
| `--role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0\|1]` | the database proxy, `execve`'d with an empty environment |
| `--seccomp=0\|1` | `BLASTIT_SECCOMP`, for the role that cannot read the environment |

`--role=dbproxy` grants nothing by itself: the credentials arrive over the pipe
the flag names, and a wheel install refuses it — the process holding them must
run no Python. http, the scheduler and a view take no flag: their place in the
process tree chooses them.

## `blastit-manifest fold`

Reads blastit's JSON events and the manifest, and lists the missing grants.

```console
$ blastit-manifest fold LOG... --manifest blastit.toml [--write] [--prefer-tables] [--allow-dangerous]
```

| Argument | What it does |
|---|---|
| `LOG`… | the logs to read; `-` is stdin |
| `--manifest PATH` (required) | the `blastit.toml` to extend |
| `--write` | rewrite the manifest in place instead of printing it |
| `--prefer-tables` | add the smallest `tables` grant for the denials |
| `--allow-dangerous` | grant statements a grant is not an allowlist for |

Not on `PATH`? `python -m blastit.fold LOG... --manifest blastit.toml` is the
same.

Exits `0` on a missing-grant diff, empty or not, `1` on an unreadable log or
manifest.
