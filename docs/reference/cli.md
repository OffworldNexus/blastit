# Command line

| Command | For |
|---|---|
| `blastit dev` | your machine, no root |
| `blastit check` | CI (`blastit test` is an alias) |
| `blastit lsp` | your editor, Linux |
| `blastit-lsp` | your editor, Linux/macOS/Windows |
| `blastit` | production |
| `blastit-manifest fold` | grants from a log |

`dev`, `check` and `blastit` read `./Blastfile` here; `BLASTIT_MANIFEST` names
another file, the only way to move it. There is no upward search.

## `blastit dev`

```console
$ blastit dev
$ blastit dev --port 9000
$ blastit dev --learn
$ blastit dev --no-reload
```

| Flag | Default | What it does |
|---|---|---|
| `--port N` | `8080`, or `PORT` | listen on `N` |
| `--host ADDR` | `127.0.0.1` | expose the server on another address; also needs `BLASTIT_DEV_EXPOSE=1`, and the boot line names the address and your uid |
| `--learn` | off | allow what the file lacks and record it; writes exact `query` lines to the `Blastfile` when it stops |
| `--no-reload` | reload on | turn live reload off: nothing is watched, and no `dev.reload.unavailable` is printed |

It runs the real process topology — http, scheduler, dbproxy and one view per
route — on your own uid, with the stage-0 secret swap and the policy checks. It
is not a sandbox: the banner says so, and the `boot` event reports
`"privilege": "unprivileged-dev"`. Logs are pretty; `BLASTIT_LOG_FORMAT` wins.
Reload is on by default in `dev` only and needs the host kernel to allow it: a
container profile that denies it gets one warning at boot, and serving continues.
A taken port exits `1` with `binding 127.0.0.1:8080: Address already in use`; pick
another with `--port`.

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
| *(none)* | — | run `pytest -q`, then check every request and statement against the `Blastfile`; an all-static file audits every `root` and runs no `pytest` |
| `--json` | off | one JSON document on stdout; everything else on stderr |
| `--learn` | off | record instead of failing, and write the missing views and exact `query` lines; an all-static file has nothing to learn (`2`) |
| `--dry-run` | off | with `--learn`: print the proposal, write nothing |
| `-- ARGS…` | — | pass `ARGS` to pytest |

`--json` prints one document on stdout: `schema_version`, `ok`, `requests`,
`statements`, `routes`, `proxies`, `findings`. Under `--learn` the document is
the proposal instead: `{"proposed": {"routes": N, "queries": N}, "wrote":
true|false}`, and `--dry-run` writes nothing (`"wrote": false`). `--learn`
replaces the `Blastfile` atomically, and only after the suite passed: a failed run
leaves it untouched ([details](../ci.md#bootstrap-an-existing-project)).

Exit codes: `0` clean, `1` findings, `2` it could not start, `3` the suite
failed (`pytest` exit `5`, no tests collected, is `3`). An all-static file
stops at `0` or `2`, with nothing run. [What it checks](../ci.md#what-it-fails-on)
and [the `--json` contract](../ci.md#for-agents-json) are on the CI page.
`blastit test` is the same command under its other name.

## `blastit lsp` (Linux)

An editor spawns it; the one thing you run by hand is `blastit lsp --help`, which
prints the usage and exits `0`. The editor starts the session with no arguments —
`blastit lsp` — and registers the extensionless name `Blastfile` for the buffers it
serves. It speaks the Language Server Protocol on stdin/stdout: the editor sends
each document, so it reads no `./Blastfile`, no config and no network, and needs no
privileges; logs go to stderr, so stdout stays the protocol. An error carries the
sentence `blastit check` prints after its `Blastfile:` prefix, and a refusal with
no position is already one sentence — grants, queries and coverage stay with
[`blastit check`](#blastit-check). Formatting rewrites the file to the [canonical
layout](blastfile.md#formatting). It exits `0` after the client's `shutdown`; `1`
when the session ends without one — the client disappears, sends `exit` first, or
its stream cannot be framed; `2` for an argument it does not take. Never because a
document is broken. This command delegates to the same implementation as the
portable [`blastit-lsp`](#blastit-lsp).

## `blastit-lsp`

The same server as [`blastit lsp`](#blastit-lsp-linux), packaged as its own binary
for Linux, macOS and Windows. An editor starts it with no arguments —
`<path>/blastit-lsp` — on stdin/stdout, and registers the extensionless name
`Blastfile`; there is no `--help` and no `--version`, and an argument is ignored
while it waits on stdin, so started by hand it exits `1` when the input ends. It
reads no `./Blastfile`, no `BLASTIT_*` variable and no configuration, opens no
socket, serves no application and holds no privilege. `blastit` itself stays the
Linux-only server: macOS and Windows get the **editor** only, because serving an app
needs Linux, as root or with the capability set ([deployments](../deploy/index.md)).
It exits `0` after the client's `shutdown`; `1` when the session ends without one —
the client disappears, sends `exit` first, or its stream cannot be framed. Never
because a document is broken.

The parser is the visible check: a `query` that does not parse gets one diagnostic
at its first token — the sentence `blastit check` prints on Linux.

Zed's extension downloads this binary itself, once per Blastit release, and
reuses the cached copy. On Linux, [`blastit lsp`](#blastit-lsp-linux) is the same
server: the extension runs it when `blastit` is on the `PATH` it sees, and downloads
otherwise. To pin a version, work offline, or use another editor, take the asset for
your target from [a release](https://github.com/OffworldNexus/blastit/releases) and
unpack it — one directory holding `blastit-lsp` (`blastit-lsp.exe` on Windows):

```text
# macOS, Apple silicon; Intel uses x86_64-apple-darwin, Linux the same command
tar -xzf blastit-lsp-aarch64-apple-darwin.tar.gz
# Windows (PowerShell)
Expand-Archive blastit-lsp-x86_64-pc-windows-msvc.zip -DestinationPath "$env:LOCALAPPDATA\blastit-lsp"
```

| Target | Asset |
|---|---|
| Linux x86_64 | `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `blastit-lsp-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple silicon | `blastit-lsp-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `blastit-lsp-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `blastit-lsp-x86_64-pc-windows-msvc.zip` |

Nothing else is built for the editor: no 32-bit target, no musl, no Windows on ARM.
Each asset is
`https://github.com/OffworldNexus/blastit/releases/latest/download/<asset>`, or the
same name under a tag. Shipped from the release that carries the portable server;
earlier tags have no `blastit-lsp-*` asset.

Keep that file anywhere permanent and name its absolute path — not its folder — in
your settings: `zed: open settings` for every project, or a project's
`.zed/settings.json`, merging:

```json
{ "lsp": { "blastit-lsp": { "binary": { "path": "/Users/you/bin/blastit-lsp" } } } }
```

On Windows that value is `C:/Users/you/bin/blastit-lsp.exe` — forward slashes are
valid JSON and paste anywhere. That object is Zed's; another LSP client
needs only the file. On Linux a `blastit` binary works instead:

```json
{ "lsp": { "blastit-lsp": { "binary": { "path": "/usr/local/bin/blastit", "arguments": ["lsp"] } } } }
```

Without `arguments` it starts the serving server and fails closed — and `"arguments"`
belongs only beside a `blastit` path, never beside `blastit-lsp`. A macOS browser
download is quarantined: `xattr -d com.apple.quarantine blastit-lsp` clears it.

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

There is no `--port` here: the listener is a [setting](settings.md) (`PORT` or
`BLASTIT_*`), and `blastit --port 8080` exits `1` with the usage.

Production **fails closed**: without root or the capability set in
[the settings](settings.md#privileges-and-the-uid-layout), and without seccomp,
it exits `1` and points at `blastit dev`. `--preset` belongs to stage 0;
`--stage=1` and `--role=…` are internal and refuse it. There is no `--version`:
the [`preload` event](log-events.md#the-three-boot-lines) carries `build`.

## `blastit-manifest fold`

Reads Blastit's JSON events and the `Blastfile`, and lists the missing grants.

```console
$ blastit-manifest fold LOG... --manifest Blastfile [--write] [--prefer-tables] [--allow-dangerous]
```

| Argument | What it does |
|---|---|
| `LOG`… | the logs to read; `-` is stdin |
| `--manifest PATH` (required) | the `Blastfile` to extend; this command does not discover it |
| `--write` | rewrite the `Blastfile` in place instead of printing it |
| `--prefer-tables` | add the smallest table grant for the denials: `select`, `insert`, `update` and `delete` lines |
| `--allow-dangerous` | also grant statements calling a function outside the safe set |

Not on `PATH`? `uv run python -m blastit.fold LOG... --manifest Blastfile`
is the same. Exits `0` on a missing-grant diff, empty or not, `1` on an
unreadable log or `Blastfile`.
