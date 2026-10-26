# Getting started

The shortest real path to a request served by blastit. It takes a couple of
minutes and needs nothing but a Linux machine and root.

!!! note "What you are about to run"

    blastit has to be root when it starts — or hold a
    [small set of capabilities](infra/anything-else.md#root-or-rootless). It
    binds the port, then drops every other process to its own unprivileged
    user. Start it as root directly, never through `sudo` in a container: App
    Platform sets `no_new_privs`, which stops `sudo`, and blastit only ever
    drops privileges.

## 1. Install it

blastit is not on PyPI yet. Wheels (Linux x86_64, CPython 3.13 and 3.14) are
built from every commit to `develop` and published on
[blastit's own package index](https://offworldnexus.github.io/blastit/simple/).
With uv:

```toml title="pyproject.toml"
[[tool.uv.index]]
name = "blastit"
url = "https://offworldnexus.github.io/blastit/simple/"
explicit = true

[tool.uv.sources]
blastit = { index = "blastit" }
```

```console
$ uv add --prerelease allow blastit
```

Like granian, blastit is a compiled extension plus a `blastit` command written
in Python, so it runs on the interpreter of the environment it is installed in:
your virtualenv and its packages are simply there.

!!! tip "Where the interpreter has to be readable"

    Routes run as `BLASTIT_UID_BASE + 1000 + i`, and blastit drops to
    `UID_BASE + 3` before it `execve`s its own database proxy. A project inside
    a `0700` directory hides both the project and its `.venv` from those users.
    `/app`, `/srv/app` or an ordinary checkout are fine.

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

`path` is a list of regular expressions, matched against the whole
[`PATH_INFO`](frameworks/index.md#routes-and-url-paths), in the order they are
written. The first route that matches wins, so a catch-all goes last.

## 3. Start it

```console
$ export BLASTIT_APP=hello:application
$ export BLASTIT_MANIFEST=$PWD/blastit.toml
$ export PORT=8080
$ sudo -E .venv/bin/blastit
{"ts":"…","level":"info","event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root","bounding_set":"dropped"}
{"ts":"…","level":"info","event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root","preset":null}
{"ts":"…","level":"info","event":"boot","component":"init","stage":"preload","immortalize":"on","immortalized":148213,"immortalize_ms":412.7,"mem_total":1057259520,"mem_used":211812352,"max_view_processes":23}
{"ts":"…","level":"info","event":"ready","component":"init","stage":"serving","routes":1,"workers":16}
```

`sudo -E` keeps your `BLASTIT_*` variables. In another terminal:

```console
$ curl -sS localhost:8080/whatever
hello from blastit
```

and blastit's stdout has grown a `request` event naming the route it chose:

```json
{"ts":"…","level":"info","event":"request","component":"scheduler","method":"GET","path":"/whatever","route":"hello","rule":0,"status":200,"outcome":"ok"}
```

`rule` is the index of the `path` regex that matched. Every request is logged
this way, including the ones no route matched — those get blastit's own `404`
and `"route": null`, and no Python runs at all. Add a catch-all `path = ['.*']`
when you would rather your app rendered its own 404s.

## 4. BLASTIT_LOG_FORMAT=pretty, for reading it yourself

```console
$ BLASTIT_LOG_FORMAT=pretty sudo -E .venv/bin/blastit
14:02:11.384 INFO  boot        stage0  seccomp=true privilege=root
14:02:11.402 INFO  request     GET /whatever → hello  200  2.1ms
```

Both formats carry the same fields; see
[log events](reference/log-events.md) for what each one means.

## 5. Where to go next

Your real app has a database, and that is where blastit earns its keep. Pick
your framework, which walks through the whole development lifecycle:

<div class="grid cards" markdown>

- :fontawesome-brands-python: **[Django](frameworks/django.md)**

    The two settings Django needs, an install, the first manifest, and the
    loop that fills in the grants from your test suite.

- :material-flask: **[Flask and other WSGI apps](frameworks/flask.md)**

    Install it the same way. What changes is how you fill in the manifest:
    [the pytest plugin is Django-only](frameworks/flask.md#no-pytest-plugin-for-flask).

- :material-tune: **[What blastit needs from an app](frameworks/index.md)**

    How routes map to URL paths, and what your app must do for its SQL to be
    checkable at all.

- :material-server-network: **[Fit it to your infra](infra/index.md)**

    A preset for the two deployments that were measured end to end, and the
    parameters for everything else.

</div>
