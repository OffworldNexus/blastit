# RESULT — MB.48

`docs/deploy/compose-traefik.md` §4 now names the capability-free binary for the
`--role=dbproxy` child, and `e2e/docs_deploy.py` asserts it cannot come back out.

## Changes

- `docs/deploy/compose-traefik.md` §4 `Dockerfile`: line 159 is now
  `ENV PYTHONUNBUFFERED=1 BLASTIT_EXEC=/usr/share/blastit/lib/blastit`; the
  paragraph below gains one clause saying `BLASTIT_EXEC` names the
  capability-free `/usr/share/blastit/lib/blastit` for the `--role=dbproxy`
  child, which cannot `execve` a capable file, while `blastit-init` keeps the
  capabilities. `/usr/share/blastit/lib` unchanged (the `.deb` path on
  `debian:trixie-slim`), `compose.yaml`'s `environment: *app-env` untouched.
- `e2e/docs_deploy.py`: `compose_checks` extracts the page's `Dockerfile` block
  with the existing `block()` helper and asserts
  `BLASTIT_EXEC=/usr/share/blastit/lib/blastit` (one new promise line, existing
  style, no new script).

## Acceptance

```
$ uv run --no-project e2e/docs_deploy.py
ok    digitalocean: the spec still attaches the cluster
docs_deploy: 47/47 documented pooler promises hold
$ echo $?
0
```

(46/46 before this node; the added `Dockerfile` promise makes 47.)

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.36s
$ echo $?
0
```

```
$ make docs-unit
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest ...
118 passed, 65 subtests passed in 1.45s
$ echo $?
0
```

## Review fixes

Three MINOR fixes from the `review-ux` round (no refactors, no other prose):

1. `e2e/docs_deploy.py` — the `Dockerfile` promise now matches the whole
   exported `ENV` line
   (`ENV PYTHONUNBUFFERED=1 BLASTIT_EXEC=/usr/share/blastit/lib/blastit`), so a
   comment mentioning the variable cannot satisfy it; same `block()` +
   `checks.that()` style, message reworded to "the image's `ENV` line exports…".
2. `e2e/docs_deploy.py` — module docstring lists the compose page's `Dockerfile`
   block among the blocks the harness reads.
3. `docs/deploy/compose-traefik.md` §4 — the re-exec sentence now uses the
   page's own noun: `BLASTIT_EXEC` names that capability-free copy for the
   `dbproxy` child; the internal `--role=` flag and `execve` are gone from user
   prose. `compose.yaml`'s `environment: *app-env`, the `ENV` line itself, and
   the page length are untouched.

```
$ uv run --no-project e2e/docs_deploy.py && make docs-check && make docs-unit
ok    compose: the image's `ENV` line exports `BLASTIT_EXEC` for the capability-free `--role=dbproxy` child
ok    digitalocean: the spec still attaches the cluster
docs_deploy: 47/47 documented pooler promises hold
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.32s
sh -n docs/install.sh
118 passed, 65 subtests passed in 1.44s
$ echo $?
0
```

## Notes

- `/usr/share/blastit/lib` kept as-is per the lead's verification (`install.sh`
  on `debian:trixie-slim` as root configures Cloudsmith APT and installs the
  `.deb` there); this node only added the missing `BLASTIT_EXEC` wiring.
