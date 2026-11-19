# Getting started

Linux, CPython 3.13 or newer, no root. Three steps to a request served by Blastit
— from a `Blastfile` you can keep in git.

## 1. Install

```console
$ uv init --app
$ uv add blastit
```

`uv add blastit` puts the `blastit` command in the project's environment, so
`uv run blastit …` always runs the version your lockfile pins.

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
  blastit dev · single-uid simulation · http://127.0.0.1:8080
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

The first line is a link: ++ctrl+click++ it (or open it) to reach the running app.
`curl` goes in a second terminal; the server keeps the first.

## What just happened

```mermaid
flowchart LR
    curl(["curl :8080"]) --> ingress["ingress<br/>method + path"]
    ingress --> sched["scheduler<br/>picks view hello"]
    sched --> view["view hello<br/>own process"]
    view --> wsgi["hello:application"]
    wsgi --> answer["200 OK<br/>hello from blastit"]
```

One request, one view, one process. Nothing in that path needed a credential, so
nothing in it holds one. [How it works](how-it-works.md) walks the same path with
a database attached.

??? note "Supported platforms & standalone binary"

    Wheels cover Linux x86_64 on CPython 3.13 and 3.14; on other Linux targets the
    sdist builds from source and needs a Rust toolchain, libclang and a C compiler.
    `uv add blastit` has no macOS or Windows build: on those, the editor comes from
    the Zed extension, which installs from a checkout until it is listed
    ([editors](local.md#editors)). Serving an app is Linux, which is what these
    steps assume.

    On Linux, without Python at all, build the standalone binary from this
    repository: `cargo build -p blastit --locked` writes `target/debug/blastit`, a
    plain Rust binary with no `libpython` to install and no `PATH` to configure.

    A PHP app is the same three steps with `entrypoint php "public/index.php"` and
    the distro's NTS `libphp` installed — the one privileged step
    ([PHP](frameworks/php/index.md)).

## Where next

* [Local development](local.md) — PostgreSQL, filling the grants in one pass,
  reading a denial.
* [CI/CD](ci.md) — one command that proves nothing was forgotten.
* [Deployments](deploy/index.md) — presets, and what production refuses.
* [Django](frameworks/python/django.md) · [Flask, SQLAlchemy and raw WSGI](frameworks/python/flask.md).
