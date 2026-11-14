# Command line

| Command | For |
|---|---|
| `blastit dev` | your machine, no root |
| `blastit check` | CI (`blastit test` is an alias) |
| `blastit` | production |
| `blastit-manifest fold` | grants from a log |

`dev`, `check` and `blastit` read `./blastit.toml` from the current directory;
`BLASTIT_MANIFEST` names another file, which is the only way to move it. There is
no upward search.

## `blastit dev`

```console
$ blastit dev
$ blastit dev --port 9000
$ blastit dev --learn
```

| Flag | Default | What it does |
|---|---|---|
| `--port N` | `8080`, or `PORT` | listen on `N` |
| `--host ADDR` | `127.0.0.1` | expose the server on another address; also needs `BLASTIT_DEV_EXPOSE=1`, and the boot line names the address and your uid |
| `--learn` | off | allow what the manifest lacks and record it; writes exact `queries` to the manifest when it stops |

It runs the real process topology — http, scheduler, dbproxy and one view per
route — on your own uid, with the stage-0 secret swap and the policy checks. It
is not a sandbox: the banner says so, and the `boot` event reports
`"privilege": "unprivileged-dev"`. Logs are pretty; `BLASTIT_LOG_FORMAT` wins.
A taken port exits `1` with `binding 127.0.0.1:8080: Address already in use`;
pick another with `--port`.

## `blastit check`

```console
$ blastit check
$ blastit check --json
$ blastit check --learn
$ blastit check --learn --dry-run
$ blastit check -- -k products
```

| Flag | Default | What it does |
|---|---|---|
| *(none)* | — | run `pytest -q`, then check every request and statement against the manifest; an all-static manifest audits every `root` and runs no `pytest` |
| `--json` | off | one JSON document on stdout; everything else on stderr |
| `--learn` | off | record instead of failing, and write the missing routes and exact `queries`; an all-static manifest has nothing to learn (`2`) |
| `--dry-run` | off | with `--learn`: print the proposal, write nothing |
| `-- ARGS…` | — | pass `ARGS` to pytest |

`--json` prints one document on stdout: `schema_version`, `ok`, `requests`,
`statements`, `routes`, `proxies`, `findings`. Under `--learn` the document is
the proposal instead: `{"proposed": {"routes": N, "queries": N}, "wrote":
true|false}`, and `--dry-run` writes nothing (`"wrote": false`). `--learn`
replaces the manifest atomically, and only after the suite passed: a failed run
leaves it untouched ([details](../ci.md#bootstrap-an-existing-project)).

Exit codes: `0` clean, `1` findings, `2` it could not start, `3` the suite
failed (`pytest` exit `5`, no tests collected, is `3`). An all-static manifest
stops at `0` or `2`, with nothing run. [What it checks](../ci.md#what-it-fails-on)
and [the `--json` contract](../ci.md#for-agents-json) are on the CI page.
`blastit test` is the same command under its other name.

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

Production **fails closed**: without root or the capability set in
[the settings](settings.md#privileges-and-the-uid-layout), and without seccomp,
it exits `1` and points at `blastit dev`. `--preset` belongs to stage 0;
`--stage=1` and `--role=…` are internal and refuse it. There is no `--version`:
the [`preload` event](log-events.md#the-three-boot-lines) carries `build`.

## `blastit-manifest fold`

Reads Blastit's JSON events and the manifest, and lists the missing grants.

```console
$ blastit-manifest fold LOG... --manifest blastit.toml [--write] [--prefer-tables] [--allow-dangerous]
```

| Argument | What it does |
|---|---|
| `LOG`… | the logs to read; `-` is stdin |
| `--manifest PATH` (required) | the `blastit.toml` to extend; this command does not discover it |
| `--write` | rewrite the manifest in place instead of printing it |
| `--prefer-tables` | add the smallest `tables` grant for the denials |
| `--allow-dangerous` | also grant statements calling a function outside the safe set |

Not on `PATH`? `uv run python -m blastit.fold LOG... --manifest blastit.toml`
is the same. Exits `0` on a missing-grant diff, empty or not, `1` on an
unreadable log or manifest.
