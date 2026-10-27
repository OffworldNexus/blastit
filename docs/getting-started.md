# Getting started

The shortest real path to a request served by blastit: a Linux machine and root,
or a small set of capabilities.

!!! warning "This is the developer-laptop path"

    The steps below assume a **regular Linux developer machine**: blastit starts
    under `sudo`, and the interpreter and the project would normally live under
    your private home. That is what makes the route uids — the unprivileged users
    blastit drops each route to — the thing you have to arrange for. On a
    **server**, in a **container** or under **systemd** none of that holds: the
    interpreter comes from the distribution or the image, a service account owns
    the code, and the setup is a different one. Start from [Fit it to your
    infra](infra/index.md) there.

## 1. A throwaway directory the route uids can read

blastit drops each route to its own uid — `BLASTIT_UID_BASE + 1000 + i`, `21000`
and up by default ([the uid
layout](reference/settings.md#privileges-and-the-uid-layout)) — and those uids are
not you and not in your group. The quickstart keeps everything they have to read
in one throwaway directory under `/tmp`:

```console
$ export BLASTIT_QUICKSTART=$(mktemp -d /tmp/blastit-XXXXXX)
$ chmod o+rx "$BLASTIT_QUICKSTART"
$ export UV_PYTHON_INSTALL_DIR="$BLASTIT_QUICKSTART/python"
$ export UV_LINK_MODE=copy       # quiet: the uv cache and /tmp are different filesystems
$ uv python install --no-bin 3.13
$ mkdir "$BLASTIT_QUICKSTART/shop" && cd "$BLASTIT_QUICKSTART/shop"
```

Every path a view imports *through* has to be readable and traversable by those
uids — the interpreter, its standard library and `site-packages`, your project and
its `.venv` — and a private home is what breaks that by default. `mktemp` makes the
directory mode `0700`, so the `chmod` opens it, and nothing in your home, to the
route uids. `UV_PYTHON_INSTALL_DIR` is what keeps `uv` from downloading the
interpreter back into `~/.local/share/uv/python`: most distributions create `~`
mode `0750`, so the route uids cannot enter it, and the failure is quiet —
libpython's built-ins keep working, then the first pure-Python import inside a view
dies (`ModuleNotFoundError: No module named 'stringprep'`), or a child cannot
`execve` the dbproxy binary. `--no-bin` keeps `uv` from leaving a `python3.13` shim
in `~/.local/bin` that would dangle once the trial is gone.

A `0755` directory inside `/tmp` is exactly the layout blastit expects: the sticky
`/tmp` above it is not a way in, because the trial directory is yours and no uid
blastit drops to may write it. (If your `/tmp` is mounted `noexec` — rare, on some
hardened systems — use a directory under `/srv` instead: the interpreter has to be
executable, not only readable.)

!!! note "When you outgrow the quickstart"

    Your own project lives somewhere permanent, probably under your home, which
    is mode `0750` and out of the route uids' reach. Then either keep the
    interpreter where every uid can read it — `/opt/uv-python`, installed the same
    way — and `chmod o+rx "$HOME"` so the uids can reach the project, or run the
    project itself from a directory that is already world-readable. blastit checks
    all of this at boot, names the first directory that blocks, and gives both
    fixes — see [Troubleshooting](troubleshooting.md#it-does-not-start).

## 2. Install blastit — as yourself, never as root

```console
$ uv init --python 3.13
$ uv venv
$ uv add blastit
```

Run every `uv` command as yourself. **Do not `sudo uv sync`, `sudo uv add` or
`sudo uv venv`**: it leaves root-owned files in `.venv` that you can no longer
update, and it buys the route uids nothing — what they need is the mode bits
(world read and traverse) and an interpreter outside your home, not a root-owned
environment. Installing is not where blastit needs root anyway; *serving* is, when
it binds the port and drops privileges.

The venv links back to the interpreter in the trial directory; confirm it does not
point into your home:

```console
$ readlink -f .venv/bin/python
/tmp/blastit-KgggGX/python/cpython-3.13.17-linux-x86_64-gnu/bin/python3.13
```

Wheels cover Linux x86_64 on CPython 3.13 and 3.14; elsewhere the sdist builds
from source and needs a Rust toolchain and libclang.

## 3. Write the smallest app and manifest

Any WSGI callable will do. This one is the whole app:

```python title="hello.py"
def application(environ, start_response):
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"hello from blastit\n"]
```

```toml title="blastit.toml"
version = 1

[[route]]
name = "hello"
path = ['.*']
```

`path` is a list of regular expressions, matched against the whole `PATH_INFO`
in the order written. The first match wins, so a catch-all goes last.

## 4. Start it

```console
$ sudo env BLASTIT_APP=hello:application \
           BLASTIT_MANIFEST=$PWD/blastit.toml PORT=8080 \
           BLASTIT_LOG_FORMAT=pretty .venv/bin/blastit
14:02:11.402 INFO  boot        stage0  seccomp=on  privilege=root  bounding_set=dropped
14:02:11.523 INFO  ready       serving  routes=1  workers=16

$ curl -sS localhost:8080/whatever
hello from blastit
14:02:11.604 INFO  request     GET /whatever → hello  200  2.1ms
```

`.venv/bin/blastit` re-executes your project's own interpreter — the one from
step 1, which is why the route uids have to be able to read it — so `hello` is
importable from the directory you started in. In a container, start blastit as
root directly: platforms set `no_new_privs`, which stops `sudo`.

Every request is logged with its route, above; a path no route matches gets
blastit's own `404`, with `"route": null` and no Python at all. The
`BLASTIT_LOG_FORMAT=pretty` line is the human format — coloured and aligned — while
the setting's own default, `json`, is one object per line for a collector to read
([log events](reference/log-events.md)).

## 5. Throw it away

```console
$ rm -rf "$BLASTIT_QUICKSTART"
```

Nothing of the quickstart is outside the trial directory — not the interpreter,
not the venv, not a root-owned file — and `/tmp` is cleared on reboot as well, so
a trial you forget about does not linger.

## Where next

* [Django](frameworks/django.md) — the settings, the first manifest, and grants
  filled in from your test suite.
* [Flask and other WSGI apps](frameworks/flask.md) — the same, without the
  pytest plugin.
* [Fit it to your framework](frameworks/index.md) — what blastit needs from any
  app.
* [Fit it to your infra](infra/index.md) — presets and parameters.
