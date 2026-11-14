---
id: MB.3.2
title: "Write the lean PHP-FPM page (Local/CI/Deploy), register it, pass dx+ai-ergonomics"
kind: task
status: ready
owner: "doc-writer"
priority: 1
depends_on: [MB.3.1]
paths:
  - docs/frameworks/php.md
  - docs/frameworks/index.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/php.review-dx.md"
  - "docs/design/php.review-ai-ergonomics.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Publish the page that defines `0.2.1`: `docs/frameworks/php.md`, PHP-FPM (`FastCGI`)
apps — Symfony / Laravel / Slim / PSR-15 `public/index.php` front controllers — with
**zero `sudo`** locally, a completeness check in CI/CD, and a fail-closed deployment
story. Accepted by the `dx` and `ai-ergonomics` critics before `MB.3.3` designs or
`MB.3.4` writes any Rust.

# Context

- Iron law: skill `dx-first`. This is the first actionable child of `MB.3`; nothing
  downstream may start before both review files end in `VERDICT: PASS`.
- Under 150 lines, opening with a working ~10-line `blastit.toml` + one command. Use
  the shape `0.2.0` already shipped for assets and let the page name it
  (`[route.static]` on `public/`, `[route.fastcgi]` for the rest) — the PHP route
  serves `public/index.php`; a `.php` entry script is never served as a file.
- Exactly three environments:
  1. **Local machine, zero `sudo`:** `uv add blastit`, the PHP app's `public/`, a
     `php-fpm` pool listening on a Unix socket the developer owns (one line, no root,
     no `/etc`), `uv run blastit dev`, then `curl -i` for (a) `/users/42` answered by
     `public/index.php` with the framework's own body, (b) `/assets/app.<hash>.css`
     answered by the static responder, (c) `/?-d+allow_url_include=1` **not** running
     PHP with those flags. Show `blastit dev --learn` writing the missing route/grant
     lines in one pass.
  2. **CI/CD:** `blastit check` and `blastit check --json` proving every declared PHP
     route was exercised, every outgoing query/grant declared, no dead grants, and
     every route with a body declares its input rule — one command, one exit code.
     Show the `--json` finding shape for an unexercised PHP route as the page's answer
     to "how do I know nothing was forgotten".
  3. **Deployments:** the same block under an existing preset
     (`blastit --preset compose-traefik`), where production refuses to start without
     capabilities; link `deploy/` and do not re-teach isolation. State the pool-worker
     default in one line and link `reference/` for the override.
- Registration, same commit: `zensical.toml` nav entry, a link from
  `docs/frameworks/index.md`, and `python/tests/runnable_docs/inventory.toml` — one
  `[[page]]` with a `[[page.fence]]` per console fence. **Every fence is accounted
  now, not faked:** the worker lands in `MB.3.4`–`MB.3.6`, so its fences use
  `mode = "example"` / `mode = "static"` with a `reason` that names the replacement
  proof (`e2e/php2shell.py` in `MB.3.7` replays this page's own commands verbatim).
  `presets` mode for the fail-closed line, exactly as `static-and-spa.md` does.
- Fresh-context critics (`dx`, `ai-ergonomics`) write
  `docs/design/php.review-dx.md` and `docs/design/php.review-ai-ergonomics.md`. Resolve
  every `BLOCKER`/`MAJOR` and list finding → fix under `## Review Resolution` in
  `RESULT.md`.

# Acceptance

```console
$ test "$(wc -l < docs/frameworks/php.md)" -le 150 && echo "page is lean"
$ test "$(grep -c '^\$ ' docs/frameworks/php.md)" -ge 6 && echo "local + CI + deploy commands shown"
$ for k in 'uv add blastit' 'blastit dev' '--learn' 'blastit check --json' 'public/index.php' 'php-fpm' 'route.fastcgi' 'route.static' 'compose-traefik'; do grep -q -- "$k" docs/frameworks/php.md || echo "missing: $k"; done
$ grep -q 'php.md' zensical.toml && grep -q '(php.md)' docs/frameworks/index.md
$ grep -q 'docs/frameworks/php.md' python/tests/runnable_docs/inventory.toml
$ make docs-unit      # published page set vs inventory: page entry and every fence present
$ make docs-check     # zensical build --strict: nav, links, anchors
$ make docs-test      # harness runs or accounts every fence; nothing unaccounted
$ for f in docs/design/php.review-dx.md docs/design/php.review-ai-ergonomics.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "both lenses pass"
$ make check
```

# Notes

- **Fence honesty.** The serving fences cannot execute before the worker exists. Do not
  invent a passing output: register them accounted with a reason that names
  `e2e/php2shell.py` and the exact assertion it makes. `make docs-test` is green now;
  `MB.3.7` proves the commands verbatim.
- **Zero kernel lectures.** No `seccomp`, `CAP_*`, uid layout, record framing or
  `openat2`. The page states the promise ("a hostile param is refused, not forwarded")
  and links `security.md` / `how-it-works.md`. Framing belongs to `MB.3.3`.
- **One canonical way.** One pool command line, one manifest shape, one front
  controller. No nginx config, no `fastcgi_split_path_info` tutorial, no WordPress.
- Do not edit `docs/frameworks/static-and-spa.md`, `docs/reference/manifest.md`,
  `docs/reference/cli.md`, or any `docs/design/*.md` other than the two review files —
  `MB.6` owns the first two right now.
- `python/tests/runnable_docs/harness.py` / `run.py` are not in this node: if a fence
  genuinely needs a new mode, account it with an existing one and log the mode as a
  follow-up instead of widening the harness.
