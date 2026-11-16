# The Blastfile

`Blastfile` is Blastit's whole routing table and permission model: one plain-text
file named `./Blastfile`.

```text title="Blastfile"
app "shop"

entrypoint python "shop.wsgi:application"

postgres $DATABASE_URL

view products GET "/products/{id:int}" {
    postgres { select products(id, name, price) }
}

view checkout POST "/checkout" {
    body json
    postgres { insert orders(id, total) }
}
```

Blastit reads `./Blastfile` from the working directory; `BLASTIT_MANIFEST` names
another path. There is no upward search, no `version` header and no `=` between a
keyword and its value (`=` inside a quoted SQL statement is data). Run
`uv run blastit fmt` once after editing: it puts the file in canonical layout and
expands a one-line block onto its own lines.

## Local development

`blastit dev` needs no `sudo` — it runs every process as you; a PHP host's `libphp` install does.

`DATABASE_URL` is your app's normal libpq URL and must be in the environment; the
[Local development](local.md) guide (0.1.1 page, rewritten in this release)
starts a throwaway one, and `uv add blastit` adds the CLI to the project.

```console
$ uv run blastit dev
```

It reads the file once — restart after an edit — binds `127.0.0.1:8080` (`--port`
moves it), and logs each request and each decision. A view with no `postgres`
block has no listener yet: run `--learn` first. A statement a granted view may not
run fails with PostgreSQL's permission error, and that log line is the statement
to grant: paste it as `query '…'`, or grant the columns it touches instead.
`BEGIN` is not a statement to paste — declare `transactions`.

Run `blastit dev --learn` to fill the grants in one pass: it allows what the file
does not cover and records it. Stop it with `Ctrl-C`, then read
`git diff Blastfile`, or replay your suite or a few `curl` calls against
`127.0.0.1:8080`. It adds the views it saw and the exact statements they ran, but
no `body` codec, `use` or `transactions` — declare those yourself. A statement
that inlines a value is refused, not written: bind the value and learn again.

## CI/CD

One unprivileged command proves nothing your suite exercised was left out:

```console
$ uv run blastit check
```

It runs your Python suite itself (`pytest -q`; `uv add --dev pytest` if the
project has none), so do not also run `pytest` yourself; arguments after `--` go
to it, and the job needs the same `DATABASE_URL` and database your suite uses. It
fails on a request no view matches, a statement no grant allows, a grant nothing
used, a view nothing reached, or a body that is not the declared codec; the full
kind list is in [the reference](reference/blastfile.md#blastit-check). `--json`
is the same document for an agent: one finding per problem, its route or path,
the SQL when there is one, and the fix.

Exit `0` clean, `1` findings, `2` could not start, `3` the suite failed. An
all-static file audits every `root` and exits `0`, or `2` naming one it cannot
read. A PHP app's gate is `uv run blastit fmt --check`, which must exit `0`; its
`check` passes the manifest, root and `libphp` preflight, then exits `2` without
pytest and `3` when pytest collects no tests — so add pytest only if you have
Python tests. A passing suite reports `no_observations`, never a clean run.

For a Python app, a clean run is the deploy gate; production enforces the same.

## Deployments

Production `blastit` fails closed: without root, the capabilities a preset names,
or seccomp, it exits instead of running less isolated. It refuses at boot; it does
not warn and continue.

```console
$ blastit --preset compose-traefik
```

On a laptop it exits `1` and names the missing `cap_*`. The preset's stack runs
the same command as a dedicated non-root uid that holds those capabilities, so
the same `Blastfile` runs in production and locally. A body sent to a view with
no `body` is a `413` in `dev` and production, not a warning. The recipes are in
[Deployments](deploy/index.md) (0.1.1 page, rewritten in this release).

Coming from `blastit.toml`? The [key map](reference/blastfile.md#migration) is
one page.

Every keyword, directive and path pattern is in the
[Blastfile reference](reference/blastfile.md).
