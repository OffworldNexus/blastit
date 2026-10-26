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

`uv add` adds to a project, so start one if you have not:

```console
$ uv init && uv add blastit       # or: pip install blastit
```

Wheels are published for Linux x86_64 on CPython 3.13 and 3.14, plus an sdist
that builds the same wheel from source anywhere else. With pip:

```console
$ pip install blastit
```

For the current `develop` instead of a release — which builds from source, and
needs a Rust toolchain (1.88+) and libclang:

```console
$ uv add "blastit @ git+https://github.com/OffworldNexus/blastit@develop"
```

CI also builds a wheel for every commit to `develop`, as an artifact of that
commit's run.

Like granian, blastit is a compiled extension plus a `blastit` command written
in Python, so it runs on the interpreter of the environment it is installed in:
your virtualenv and its packages are simply there.

!!! tip "Where the project has to be readable"

    Routes run as `BLASTIT_UID_BASE + 1000 + i`, and blastit drops to
    `UID_BASE + 3` before it `execve`s its own database proxy — so the project
    and its `.venv` have to be readable by those users. A project inside a
    `0700` directory hides both. `/app`, `/srv/app` or an ordinary checkout are
    fine.

    <!-- E2b -->
    An interpreter whose own standard library sits behind a private home
    directory (`uv python install` puts one in `~/.local/share/uv/python`, and
    `~` is often `0750`) is refused at boot, naming the interpreter and the fix,
    rather than failing later with `ModuleNotFoundError: No module named
    'stringprep'` inside a view. A world-traversable interpreter — the
    distribution's own `/usr/bin/python3.x`, or a flat install under `/opt` — is
    what to use.

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
$ sudo env BLASTIT_APP=hello:application \
           BLASTIT_MANIFEST=$PWD/blastit.toml \
           PORT=8080 \
           .venv/bin/blastit
{"ts":"…","level":"info","event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"root","bounding_set":"dropped","preset":null}
{"ts":"…","level":"info","event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root","preset":null}
{"ts":"…","level":"info","event":"boot","component":"init","stage":"preload","immortalize":"on","immortalized":148213,"immortalize_ms":412.7,"mem_total":1057259520,"mem_used":211812352,"max_view_processes":23}
{"ts":"…","level":"info","event":"ready","component":"init","stage":"serving","routes":1,"workers":16}
```

<!-- E2b -->
`BLASTIT_APP` is importable from the directory you start blastit in, so `hello`
is found because you are in the project: no `PYTHONPATH` and no installed
package are needed. (A project that *is* installed — the usual case for an app
behind `manage.py` — resolves the same way, from the console script's own
import path.)

`sudo env VAR=…` passes exactly those variables to root, which is what always
works: `sudo -E` is ignored when sudo is configured with `env_reset`. In a
container, start blastit as root directly — no `sudo` at all — because a
platform sets `no_new_privs`, which stops `sudo` from working. In another
terminal:

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
$ sudo env BLASTIT_LOG_FORMAT=pretty BLASTIT_APP=hello:application \
           BLASTIT_MANIFEST=$PWD/blastit.toml PORT=8080 .venv/bin/blastit
14:02:11.384 INFO  boot        stage0  seccomp=true privilege=root
14:02:11.402 INFO  request     GET /whatever → hello  200  2.1ms
```

<!-- E2b -->
`pretty` prints the same events as the JSON format, child events like `request`
and `sql.denied` included, as one coloured line each; see
[log events](reference/log-events.md) for what each field means.

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
