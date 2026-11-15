# The PHP leg: a real `libphp` and the `php2shell` matrix

Proves on a live runtime what `docs/frameworks/php.md` promises and what
`ADR-0010` claims: a Symfony/Laravel-shaped `public/index.php` runs embedded
under `blastit dev` with zero `sudo`, and every `php2shell` row fails
harmlessly. `make e2e` runs it (the `php` service in `e2e/compose.yaml`, driven
by `e2e/php2shell.py`); nothing here is a separate command to remember.

```
e2e/php/
├── Dockerfile        the runtime: the built wheel + Debian's NTS libphp8.4 + the shim
├── fixture/          what blastit serves (copied to a scratch dir by the driver)
│   ├── blastit.toml  two [route.php] routes: the API above the catch-all
│   ├── public/       the front controller, the hashed stylesheet
│   ├── api/          the second route's entry (no [route.static])
│   ├── src/          the twelve $_SERVER facts, shared
│   ├── canaries/     prepend.php (PREPEND-RAN), shell.jpg (JPG-RAN)
│   └── elsewhere/    evil.php (EVIL-RAN), outside the static root
├── probes.md         the accepted §4.7 refusal matrix (verbatim, see below)
└── entry_probe.py    one GET, for e2e/rootless's entry-swap check
```

## Why this image, and what it does and does not claim

`libphp` is not in upstream's `php` docker images (they are built without
`--enable-embed`), so the runtime comes from **Debian trixie's
`libphp8.4-embed` 8.4.27**, exactly what a host installs with
`apt install libphp-embed` (`docs/frameworks/php.md` §1). The base image is
pinned by digest (`python:3.14-slim-trixie@sha256:9aaa…`, the CPython the
prebuilt wheel targets) and the package by version, so the run is reproducible.

**The claim is that the surface is absent by construction, not that an
unpatched runtime would be safe.** There is no `php-cgi` binary, no FastCGI
parameter stream, no argv, and no INI channel a request can reach: the C shim
builds `$_SERVER` and the `ZEND_INI_SYSTEM` blob itself. `8.4.27` is Debian's
current package and this suite does not pretend otherwise — no distribution
ships an unpatched `libphp` an 8.4 shim could load, so "even an unpatched
runtime" is neither claimed nor testable here.

What *is* tested about the runtime is `P4b`: the same interpreter, the same
canary file `canaries/prepend.php` that row 7 names, `php -d
auto_prepend_file=…` — its canary is **present**, so "the canary is absent"
through `blastit` means the channel is absent, not that the canary is broken.
Rows 9 and 10 are tripwires for a port (on Linux NTS nothing can make them
fail: no Windows best-fit, no argv) and say so in their note.

## `probes.md` is a copy, on purpose

The matrix lived in §4.7 of `docs/design/php-fastcgi.md`, the design `ADR-0010`
replaced; `MB.3.32` deleted that corpus (18 files) and kept the research. The
`E`/`php2shell #` columns are the accepted, reviewed threat list, so they are
copied here verbatim rather than re-derived — `php2shell.py --self-test` parses
this file and fails if `PROBES` stops matching it row for row (24 reachable rows,
`E1`–`E17` + `E19`–`E25`; the attack column's union is exactly `#1`–`#20`).

The three columns that describe the *old* mechanism (`Blastit handling`,
`Outcome`, `Owner`) are kept for the record; the driver asserts the
embedded-era observable each row now has, and the row's `note` says why when the
status differs (no CGI head exists to malform, and the FastCGI param budget
became the HTTP head limit).

## Running it alone

```console
$ uv run --no-project e2e/php2shell.py --self-test
$ make e2e                      # the whole harness, this leg included
```

Inside an image you already built, the driver is its own entry point — it starts
`blastit dev` itself, so `127.0.0.1:8080` is the page's own address:

```console
$ docker run --rm -v "$PWD/e2e/php2shell.py:/opt/e2e/php2shell.py:ro" \
    -v "$PWD/e2e/php/probes.md:/opt/e2e/probes.md:ro" \
    -v "$PWD/docs/frameworks/php.md:/opt/e2e/php.md:ro" blastit-e2e-php \
    python /opt/e2e/php2shell.py --fixture /fixture --site /tmp/site \
    --page /opt/e2e/php.md --matrix /opt/e2e/probes.md
```

## What it does not prove

`[route.php] entry` is a path the worker resolves per request; only `libphp` and
the shim are pinned by descriptor (`ADR-0010` §1). A decoy swap *inside* the
application root therefore executes — `e2e/rootless/php_entry_swap.py` prints
that measurement as **MB.3.56's witness** (that node pins the entry by
`(st_dev, st_ino)`), not as an accepted behaviour. The guarantee that holds today,
and that the same check asserts, is that the manifest directory is not writable
by the route's uid (`ADR-0010` §2): `chmod`/`O_CREAT`/`unlink` all answer
`EPERM`/`EACCES`.

The three commands on `docs/frameworks/php.md`: one is executed verbatim
(`blastit dev --learn`) and two are explained (a published-wheel install and a
production preset owned by `e2e/rootless`).
