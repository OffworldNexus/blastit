# Getting started

The shortest real path to a request served by blastit: a Linux machine and root,
or a small set of capabilities.

## 1. Install

```console
$ uv init && uv add blastit
```

Wheels cover Linux x86_64 on CPython 3.13 and 3.14; elsewhere the sdist builds
from source and needs a Rust toolchain and libclang.

!!! warning "Readable by the route uids"

    Routes run as uids `BLASTIT_UID_BASE+1000+i`, so the project, its `.venv` and
    the interpreter must be readable and traversable by those uids. blastit
    checks this at boot and refuses, naming the first directory that blocks and
    both fixes: `UV_PYTHON_INSTALL_DIR=/opt/uv-python` before
    `uv python install`, or `chmod o+rx` that directory and every one above it.

## 2. Write the smallest app and manifest

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

## 3. Start it

```console
$ sudo env BLASTIT_APP=hello:application \
           BLASTIT_MANIFEST=$PWD/blastit.toml PORT=8080 .venv/bin/blastit
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root"}
{"event":"ready","component":"init","stage":"serving","routes":1,"workers":16}

$ curl -sS localhost:8080/whatever
hello from blastit
```

`.venv/bin/blastit` re-executes your project's own interpreter, so `hello` is
importable from the directory you started in. In a container, start blastit as
root directly: platforms set `no_new_privs`, which stops `sudo`.

Every request is logged with its route. A path no route matches gets
blastit's own `404`, with `"route": null` and no Python at all.

## 4. Make the log readable

Add `BLASTIT_LOG_FORMAT=pretty` to the same command and the same events arrive
as text:

```console
14:02:11.402 INFO  request  GET /whatever → hello  200  2.1ms
```

## Where next

* [Django](frameworks/django.md) — the settings, the first manifest, and grants
  filled in from your test suite.
* [Flask and other WSGI apps](frameworks/flask.md) — the same, without the
  pytest plugin.
* [Fit it to your framework](frameworks/index.md) — what blastit needs from any
  app.
* [Fit it to your infra](infra/index.md) — presets and parameters.
