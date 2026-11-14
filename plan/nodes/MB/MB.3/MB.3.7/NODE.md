---
id: MB.3.7
title: "Prove FastCGI, the documented commands and the php2shell suite end-to-end in Docker"
kind: task
status: ready
owner: implementer
priority: 4
depends_on: [MB.3.6]
paths: ["e2e/php/**", e2e/php2shell.py, e2e/compose.yaml, e2e/run.py]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Prove on a live, **unpatched** `php-fpm` that the epic's promises hold: a Symfony/Laravel
style `public/index.php` front controller runs under `blastit dev` and `blastit check`
with zero `sudo`, and every `php2shell` probe in the design's refusal matrix fails
harmlessly. Replay `docs/frameworks/php.md`'s own commands verbatim, in a container,
against the shipped wheel.

# Context

- Everything is proven here, not taken on trust: the page (`MB.3.2`), the design's
  refusal matrix (`MB.3.3`), the codec (`MB.3.4`), the grammar (`MB.3.5`) and the
  session/CLI (`MB.3.6`).
- New fixture under `e2e/php/`: a pinned **unpatched** `php:7.4-fpm`-style image (record
  the digest and why that tag; the point is that the runtime is not what saves us), a
  non-root pool listening on a Unix socket in a writable dir, `public/index.php` that
  returns JSON naming the framework-style route it resolved (`/users/42` →
  `{"route":"user.show","id":"42"}`), a `public/assets/app.<12 hex>.css` for the static
  responder, and a **canary**: a file the front controller would write only if an
  injected `auto_prepend_file` ran (or any RCE gadget executed). The canary must be
  absent at the end of the run.
- Driver `e2e/php2shell.py` (stdlib only, same shape as `e2e/static_assets.py`):
  - `--self-test` parses the attack table out of `docs/frameworks/php.md` /
    `docs/design/php-fastcgi.md` and asserts this driver names every row;
  - replays every `$ ` command of `docs/frameworks/php.md` verbatim and asserts the
    documented status/body shape (a page command that drifted is a failure, not a note);
  - sends the attack set and asserts each one is harmless **and** that the response is
    the front controller's own JSON, never `phpinfo`, a stack trace, or source:
    `SCRIPT_FILENAME` override and traversal (`/../../etc/passwd`), `PATH_INFO` into
    `SCRIPT_FILENAME`, `PHP_VALUE` / `PHP_ADMIN_VALUE` request headers,
    `PHP_ADMIN_VALUE: auto_prepend_file=...`, `allow_url_include=1` +
    `auto_prepend_file=php://input`, CVE-2012-1823 `?-d+allow_url_include=1+-d+auto_prepend_file=php://input`,
    the CVE-2024-4577 best-fit/encoding probe, `REDIRECT_STATUS` / `DOCUMENT_ROOT`
    spoofing, `Proxy:` → `httpoxy` (`HTTP_PROXY` must not reach PHP; assert with a
    front controller that echoes `getenv('HTTP_PROXY')` on a debug path that only the
    e2e manifest grants), duplicate/unknown params, and a `*.php` file request that must
    not be served as source.
  - asserts the static responder still answers `public/assets/*` and that `check`
    passes with a PHP route exercised by the driver.
- Registration: one `php` service in `e2e/compose.yaml` and one leg in `e2e/run.py`,
  additive to the existing static/sqlalchemy/rootless legs, so bare `make e2e` covers
  it. Keep the leg's wall time small (reuse the prebuilt wheel image pattern).

# Acceptance

```console
$ uv run --no-project e2e/php2shell.py --self-test
$ grep -q 'docs/frameworks/php.md' e2e/php2shell.py && echo "driver replays the page"
$ for k in SCRIPT_FILENAME PHP_VALUE PHP_ADMIN_VALUE auto_prepend_file CVE-2012-1823 CVE-2024-4577 httpoxy HTTP_PROXY REDIRECT_STATUS; do grep -q "$k" e2e/php2shell.py || echo "missing probe: $k"; done
$ grep -q 'php' e2e/compose.yaml && grep -q 'php2shell' e2e/run.py
$ make e2e
$ make check
```

# Notes

- **Coordination:** `MB.8` also lands an `e2e/**` leg (Django `collectstatic`). Both
  edits to `e2e/compose.yaml` and `e2e/run.py` are additive — rebase, keep their
  service and leg, never reorder or delete theirs. If the merge is ambiguous, stop and
  record it rather than choosing a side.
- Docker only, no network installs beyond what other legs already cache; a PHP image
  pull is the one new artifact and must be pinned.
- Do not weaken a probe to make it pass: if a payload succeeds, that is the finding —
  `plan.py --agent implementer human add "…" --node MB.3.7`, and record the
  reproduction in `RESULT.md`. Never edit a `critical_*.rs` or the design matrix to
  match observed behaviour.
- If the page's commands cannot run as written (a flag the CLI does not have), that is
  a page defect for `MB.3.2` — log it as a follow-up instead of inventing a command in
  the driver.
- No product code in this node: `crates/**` is out of scope. A real defect found here
  becomes its own node.
