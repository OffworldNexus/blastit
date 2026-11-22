# Getting started

Blastit is a **Linux** application server: three steps from a `./Blastfile`, no root, no `sudo`.
One binary serves Python WSGI, embedded NTS PHP (8.3, 8.4, 8.5 and 8.6) and static sites.

## 1. Install

```console
$ curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh
```

The installer adds the signed Blastit APT (Debian/Ubuntu) or RPM (Fedora/RHEL/Rocky/Alma)
repository and installs `blastit` plus a shim per runtime it finds, or the archive under `~/.local`.

!!! warning "musl hosts need one more package"

    A musl wheel and the musl archive load a Rust runtime that needs `apk add libgcc`
    (`_Unwind_*` symbols are not in musl).

??? note "Escape hatches — one line each"

    * `--nightly` — the nightly repository component instead of the releases.
    * `--user` — no root: the archive under `~/.local`, then the `PATH` line it prints.
    * `--develop [ref]` — the branch build CI made, with the digest you pass in `BLASTIT_SHA256`.
    * Alpine/musl, or neither `apt-get` nor `dnf` — the installer takes the `.tar.gz` itself;
      every flag: `curl -fsSL …/install.sh | sh -s -- --help`.

### Or pin it in your project lockfile

Per-project pins are theirs to upgrade: `uv lock --upgrade` / `composer update`.

```console
$ uv init --app
$ uv add --dev blastit
```

```console
$ composer require --dev offworldnexus/blastit
```

The global `blastit` hands `dev`, `check`, `fmt` and `lsp` to `.venv/bin/blastit`,
`vendor/bin/blastit` or `node_modules/.bin/blastit`, so a pin runs without a prefix.

## 2. Write two files

=== "Python (WSGI)"
    ```python title="hello.py"
    def application(environ, start_response):
        start_response("200 OK", [("Content-Type", "text/plain")])
        return [b"hello from blastit\n"]
    ```

    ```text title="Blastfile"
    app "hello"

    entrypoint python "hello:application"

    view hello GET "/*" {
    }
    ```

=== "PHP"
    ```text title="public/index.php"
    <?php
    header("Content-Type: text/plain");
    echo "hello from blastit\n";
    ```

    ```text title="Blastfile"
    app "hello"

    entrypoint php "public/index.php"

    static "/" {
        root "public"
    }

    view app GET "/*" {
    }
    ```

=== "Static & SPA"
    ```text title="public/index.html"
    <!doctype html>
    <title>hello</title>
    <p>hello from blastit</p>
    ```

    ```text title="Blastfile"
    static "/" {
        root "public"
    }
    ```

`entrypoint python` names the WSGI callable; `entrypoint php` the front controller.

## 3. Run it

```console
$ blastit dev
  blastit dev · single-uid simulation · http://127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
  view  hello  GET, HEAD  /.*
```

`blastit dev` reads `./Blastfile`, forks production's processes on one uid, and prints a
clickable link — one request, one view, one process, no credential in that path. `blastit
check` is the gate that proves nothing was forgotten ([CI/CD](ci.md)).

```mermaid
flowchart LR
    curl(["curl :8080"]) --> ingress["ingress<br/>method + path"]
    ingress -- "scheduler" --> view["view hello<br/>own process"]
    view --> app["hello:application<br/>200 OK"]
```

??? note "Linux targets, one wheel, one binary"

    Linux x86_64 and aarch64, glibc and musl: one `py3-none` wheel per platform runs
    `blastit check`, `fmt` and `fold` on any CPython 3.12+; serving WSGI needs CPython
    3.13 or 3.14 on the host. The same binary serves PHP on the NTS `libphp` and static sites.

## Where next

* [The Blastfile](blastfile.md) — views, grants and one-line edits.
* [Install reference](install.md) — package manager, repositories, upgrade.
* [Local](local.md) · [CI/CD](ci.md) · [Deployments](deploy/index.md).
