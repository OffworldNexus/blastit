<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/brand/header-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/brand/header-light.svg">
  <img src=".github/brand/header-light.svg" alt="blastit — Small bird. Small blast radius." width="960">
</picture>

<p align="center">
  <a href="https://offworldnexus.github.io/blastit/">Documentation</a> ·
  <a href="https://offworldnexus.github.io/blastit/getting-started/">Get started</a> ·
  <a href="ARCHITECTURE.md">Architecture</a> ·
  <a href="LICENSE">Apache 2.0</a>
</p>

**A WSGI server that keeps the blast radius of a security bug small.**

In a typical Python web app every view runs in the same process, with the same
database credentials and the same rights: one exploitable bug in the least
important page and the attacker holds everything the app holds. blastit serves
the same app, unchanged, but splits it up. Each route gets its own process under
its own Unix user; each route talks to a local proxy instead of the real
database, so the real URL never reaches your code; and each route may run only
the SQL statements its manifest grants it. If the catalogue page is compromised,
the attacker can list products — and nothing else.

| One sandbox per route | Secrets stay out of reach | Only the grants it needs |
| --- | --- | --- |
| Each route runs in its own process, under its own Unix user. | Your app talks to local proxies; only blastit holds the real credentials. | Every route may run only the SQL statements its manifest grants. |

Your framework doesn't know any of this happens. It gets ordinary WSGI requests
and routes them itself. All blastit adds is one file, `blastit.toml`.

> **Status: pre-release.** blastit runs a real Django app end to end on
> DigitalOcean App Platform in CI, but it hasn't had a stable release, and the
> manifest format may still change. Linux only, CPython 3.13+.

## Get started

```console
$ uv add blastit                  # or: pip install blastit
```

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

```console
$ export BLASTIT_APP=hello:application BLASTIT_MANIFEST=$PWD/blastit.toml \
         PYTHONPATH=$PWD PORT=8080
$ sudo -E .venv/bin/blastit
```

A request to `http://localhost:8080/` is now served by a sandboxed process, and
blastit's stdout has one `request` event per request naming the route it chose.
It has to start as root — it binds the port and then drops every other process
to its own unprivileged user — and it wants to be PID 1 in a container. That is
the whole of the setup.

## Read the documentation

**[Fit it to your framework](https://offworldnexus.github.io/blastit/frameworks/)**
takes Django, Flask or any other WSGI app from an existing project to a manifest
filled in from your test suite, and then through day-to-day life with it.

**[Fit it to your infra](https://offworldnexus.github.io/blastit/infra/)** covers
`--preset digitalocean-paas` and `--preset compose-traefik`, what each value
assumes, full copy-pasteable examples, and the settings to combine for anything
else — Kubernetes, a plain VM, nginx in front.

**[The reference](https://offworldnexus.github.io/blastit/reference/settings/)**
has every `BLASTIT_*` setting, the CLI, the manifest format key by key, the log
events, and the pytest plugin.

**[How it works](https://offworldnexus.github.io/blastit/how-it-works/)** is the
tour, and
**[the security model](https://offworldnexus.github.io/blastit/security/)** says
what the isolation does and does not claim.

## Development

```console
$ cargo fmt --all -- --check
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo test --workspace
```

`develop` is the only long-lived branch. Each roadmap step in
[`ROADMAP.md`](ROADMAP.md) lands as one pull request from a
`feature/<step>-<slug>` branch, with green CI before merging.

## License

Apache 2.0 — see [`LICENSE`](LICENSE).
