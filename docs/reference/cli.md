# Command line

blastit has almost no command line. Configuration is the environment, and the
only argument an operator ever needs is `--preset`.

## `blastit`

```console
$ blastit
$ blastit --preset digitalocean-paas
$ blastit --preset=compose-traefik
$ blastit --help
```

| Argument | What it does |
|---|---|
| *(none)* | runs the server, reading its configuration from the environment |
| `--preset NAME`, `--preset=NAME` | supplies a set of defaults for one platform; every `BLASTIT_*` variable that is set (and `PORT`) still wins. The last `--preset` wins if you write two |
| `--help`, `-h` | prints the usage and the list of presets, with one line of description each |

`--preset` is a stage 0 argument and is refused with `--stage=1` or
`--role=…`: a process below stage 0 has no configuration of its own to apply a
preset to.

There is **no `--version`**: the version is in the wheel's metadata
(`uv pip show blastit`, `pip show blastit`), and the boot log's
[`preload` event](log-events.md#the-three-boot-lines) is what identifies the
build by its behaviour.

### Where the script comes from

`blastit` is the project's console script, so it lives in the environment the
wheel was installed into:

```console
$ .venv/bin/blastit          # the same thing, unambiguously
$ uv run blastit             # through uv, so the project's environment is the one used
$ python -m blastit          # when only the package is importable
$ sudo -E .venv/bin/blastit  # root, keeping the environment, for a local run
```

The script re-executes *that* interpreter for stage 1, which is what makes your
project's packages importable in every view process — so which interpreter it is
matters more here than for an ordinary console script.

## Internal arguments

These exist because a process has to be able to say which role it is playing and
which descriptors it inherited. They are not for operators, and a mistake with
them is a `blastit:` line on stderr and exit status `1`.

| Argument | Role |
|---|---|
| `--stage=1 --config-fd=N [--dbproxy-pid=P --dbproxy-events-fd=N --dbproxy-stdout-fd=N --dbproxy-stderr-fd=N]` | stage 1: the application environment, after the `execve` barrier |
| `--role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0\|1]` | the database proxy, `execve`'d with an empty environment |
| `--seccomp=0\|1` | the same as `BLASTIT_SECCOMP`, for the one role that is `execve`'d and so cannot read it from its environment. Accepted for `--role=dbproxy` only |

**`--role=dbproxy` grants nothing by itself.** The real credentials arrive over
the pipe the flag names, so a process started by hand with this flag has no
pipe, no credentials and no way to get either. The other roles — http, the
scheduler, a view — have no flag at all: they are chosen by their position in
the process tree.

A wheel install refuses `--role=dbproxy` outright
(`--role=dbproxy runs no interpreter; use the blastit-dbproxy binary this
installation ships`), because the process holding the credentials must run no
Python. The standalone binary plays the role itself.

## `blastit-manifest fold`

The manifest tooling, next to the server. It reads blastit's JSON event stream
and the manifest, and says what grants are missing.

```console
$ blastit-manifest fold LOG... --manifest blastit.toml [--write] [--prefer-tables] [--allow-dangerous]
```

| Argument | What it does |
|---|---|
| `LOG` (one or more, required) | a blastit log to read; `-` reads standard input |
| `--manifest PATH` (required) | the `blastit.toml` to extend |
| `--write` | rewrite the manifest in place instead of printing the TOML |
| `--prefer-tables` | add the smallest `[route.<proxy>.tables]` grant covering the denied statements, keeping as exact queries whatever a table grant cannot describe |
| `--allow-dangerous` | grant statements a grant is not an allowlist for, instead of refusing them |

It exits `0` when it printed (or wrote) what the manifest was missing — an empty
diff is a normal, good outcome — and `1` on anything it could not do: an
unreadable log, a manifest it cannot parse, edit, or prove safe.

```console
$ python -m blastit.fold LOG... --manifest blastit.toml
```

does the same where only the package is importable. What it refuses and why is
in [day-to-day life](../frameworks/day-to-day.md#fold-the-log).

## See also

* [Settings](settings.md) — the environment, which is the real command line.
* [Getting started](../getting-started.md) — the two commands that serve a
  request.
