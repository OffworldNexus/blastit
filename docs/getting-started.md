# Getting started

Linux, CPython 3.13 or newer, no root. Three steps to a request served by Blastit.

## 1. Install

```console
$ uv init --app
$ uv add blastit
```

Wheels cover Linux x86_64 on CPython 3.13 and 3.14; elsewhere the sdist builds
from source and needs a Rust toolchain and libclang.

Without Python at all, build the standalone binary from this repository:
`cargo build -p blastit --locked` writes `target/debug/blastit`, a plain Rust
binary with no `libpython` to install and no `PATH` to configure.

## 2. Write two files

Any WSGI callable works. This one is the whole app:

```python title="hello.py"
def application(environ, start_response):
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"hello from blastit\n"]
```

```text title="Blastfile"
app "hello"

entrypoint python "hello:application"

view hello GET "/*" {}
```

`entrypoint python` names the WSGI callable, `module:attribute`. `view hello GET
"/*"` matches every path and answers `GET`; views are tried in the order written,
so a specific view goes above a catch-all.

## 3. Run it

```console
$ uv run blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

```console
$ curl localhost:8080/whatever
hello from blastit
```

`blastit dev` reads `./Blastfile` and runs as you: no `sudo`, no setup, no
environment variables. It forks the same processes production does, on one uid,
so a real request takes a real route. A path no view matches gets Blastit's own
`404`, with no Python at all. `blastit dev --port N` moves it off `8080`.

## Where next

* [Local development](local.md) — PostgreSQL, filling the grants in one pass,
  reading a denial.
* [CI/CD](ci.md) — one command that proves nothing was forgotten.
* [Deployments](deploy/index.md) — presets, and what production refuses.
* [Django](frameworks/django.md) · [Flask and other WSGI apps](frameworks/flask.md).
