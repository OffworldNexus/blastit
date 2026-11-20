# Command line

| Command | For |
|---|---|
| `blastit dev` | your machine, no root |
| `blastit check` | CI (`blastit test` is an alias) |
| `blastit fmt` | canonical `Blastfile` formatting (`--check` in CI) |
| `blastit lsp` | your editor, Linux |
| `blastit-lsp` | your editor, Linux/macOS/Windows |
| `blastit help [COMMAND]` | the usage text for one command (`dev`, `check`, `fmt`, `lsp`) |
| `blastit` | production |

`dev`, `check`, `fmt` and `blastit` read `./Blastfile` here; `BLASTIT_MANIFEST`
names another file, the only way to move it. There is no upward search.

Every example below writes `blastit`. One binary serves Python, PHP and static, and the
installer's package — or a `uv add --dev blastit` pin — puts that name on `PATH`. A project
that pinned its own copy still runs it: `dev`, `check`, `fmt` and `lsp` hand off to
`./.venv/bin/blastit`, `./vendor/bin/blastit` or `./node_modules/.bin/blastit` when one is
present and safe, and `BLASTIT_NO_LOCAL_HANDOFF=1` keeps the global binary for every command
([which binary answers](#blastit)).

Colour is decided **per stream**: a stream that is not a terminal, a non-empty
`NO_COLOR`, or `TERM=dumb` gets plain text, so `blastit --help >help.txt` writes
none while the refusal it prints on stderr keeps its own.

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

A flag `dev` does not take is one sentence on stderr and exit `1`;
`blastit check` (and `test`) answer the same class of mistake with `2`.

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
| `--learn` | off | record instead of failing, and write missing unrouted views and their exact `query` lines (`dev --learn` adds statements to existing views); an all-static file has nothing to learn (`2`) |
| `--dry-run` | off | with `--learn`: print the proposal, write nothing |
| `-- ARGS…` | — | pass `ARGS` to pytest; Blastit's own `--blastit-manifest` and `--blastit-live-server` are refused (`2`) |

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

## `blastit fmt`

`blastit fmt [PATH]` rewrites `./Blastfile` (or `PATH`) to its [canonical
layout](blastfile.md#formatting), preserving comments; `blastit fmt --check
[PATH]` writes nothing and exits `0` when the file is canonical, `1` when it
needs formatting, or `2` when the file does not parse or cannot be read.

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
for Linux, macOS and Windows. It is a console script of the `blastit` package (and of the
`py3-none` wheel), so `blastit-lsp --version` prints one line and exits `0` — the probe a
package manager or an editor extension makes before it spawns anything. An editor starts
it with no arguments — `<path>/blastit-lsp` — on stdin/stdout, and registers the
extensionless name `Blastfile`; any other argument is one sentence on stderr and exit `2`,
so started by hand it waits on stdin and exits `1` when the input ends. It
reads no `./Blastfile`, no `BLASTIT_*` variable and no configuration, opens no
socket, serves no application and holds no privilege. `blastit` itself stays the
Linux-only server: macOS and Windows get the **editor** only, because serving an app
needs Linux, as root or with the capability set ([deployments](../deploy/index.md)).
It exits `0` after the client's `shutdown`; `1` when the session ends without one —
the client disappears, sends `exit` first, or its stream cannot be framed. Never
because a document is broken.

The parser is the visible check: a `query` that does not parse gets one diagnostic
at its first token — the sentence `blastit check` prints on Linux.

Zed's extension finds the server itself, first match wins: an
`lsp.blastit-lsp.binary.path` you set, then `blastit-lsp` on the `PATH` it sees,
then `blastit lsp` there, and last this release asset for your platform, unpacked
once and then reused from disk — a later start needs no network at all. To pin a
version, work offline, or use another editor, take the asset for your target from
[a release](https://github.com/OffworldNexus/blastit/releases) and unpack it — one
directory holding `blastit-lsp` (`blastit-lsp.exe` on Windows):

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
`zed: open settings`, merging:

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
| `-V`, `--version` | prints one line, `blastit <version>`, and answers only when the command line carries nothing else; `--help` and `--version` always answer from the global `blastit` |

There is no `--port` here: the listener is a [setting](settings.md) (`PORT` or
`BLASTIT_*`), and `blastit --port 8080` exits `2`, naming `--port` and pointing at
`PORT=<n>`; the `-p` spelling is refused the same way.

Production **fails closed**: without root or the capability set in
[the settings](settings.md#privileges-and-the-uid-layout), and without seccomp,
it exits `1` and points at `blastit dev`. `--preset` belongs to stage 0;
`--stage=1` and `--role=…` are internal and refuse it.

### Which binary answers { #which-binary }

`dev`, `check`, `fmt` and `lsp` run the project's own `blastit` when
`./.venv/bin/blastit`, `./vendor/bin/blastit` or `./node_modules/.bin/blastit` is present
and safe. An activated `$VIRTUAL_ENV` names the first candidate instead of the `./.venv`
beside the working directory. An ambient candidate that fails a safety check is **skipped**
with one warning line — an unusual checkout or a third-party `node_modules` never bricks
the command — and probing continues with the next candidate, then the global binary.
`BLASTIT_BIN` is the one path you name yourself: it is probed last, and a failure there is
a **refusal** (`1`), not a skip, because you pointed at it. `BLASTIT_NO_LOCAL_HANDOFF`
keeps the global binary for every command. `Serve` never hands off: `--version` and
`--help` answer from the binary you typed.
