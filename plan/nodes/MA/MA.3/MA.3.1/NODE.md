---
id: MA.3.1
title: "Runnable-docs harness executing README and docs snippets unprivileged"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MA.2]
paths: [Makefile, "python/tests/runnable_docs/**"]
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-12T19:16"
updated: "2026-11-13T00:51"
---

# Goal

Add an unprivileged, Docker-free, deterministic **runnable-docs harness** that executes the Getting Started, local `blastit dev` / `--learn`, and `blastit check` console snippets from `README.md` and `docs/` verbatim against a freshly built wheel, exposed as `make docs-test` and wired into `make check` (milestone item 5).

# Context

- `MA.2` just landed `dev`, `--learn` and `check`; the docs written in `MA.1` can now be executed.
- Snippets live in triple-backtick `console` fences. Build with `scripts/build-wheel.sh`, install into a throwaway venv, run each fence in its own temp project directory from a clean environment (`PATH`, `HOME`, no inherited `BLASTIT_*`).
- No Docker, no `sudo`, no host PostgreSQL required: DB-dependent snippets (`--learn`, `blastit check`) run against a deterministic in-harness PostgreSQL wire stub, or `BLASTIT_TEST_PG_URL` when a real server is provided. Never `skip`/`xfail` a snippet.
- Load skills `python-standards`, `testing`.

# Acceptance

```console
$ make docs-test          # 0 skipped, 0 xfailed, every executed fence PASS
$ make docs-test          # second run: same verdicts (determinism)
$ grep -rnE '\bskip|xfail' python/tests/runnable_docs   # no masking construct
$ test "$(id -u)" -ne 0 && make docs-test               # passes unprivileged
$ make check                                            # includes docs-test
```

- `python/tests/runnable_docs/inventory.toml` maps **every** `console` fence in `README.md` and the public `docs/` pages to either an executed command or a written reason; the inventory test fails when a fence or page is unaccounted for (add a fence -> red).
- Deployment recipes (`docs/deploy/*`) are not executed but are statically checked (TOML parses, every `blastit`/`docker` command in the fence exists) and recorded as such.
- `RESULT.md` lists the harness-found drift as findings for `MA.3.2`.

# Notes

- Owns `Makefile` and `python/tests/runnable_docs/**` only. Do **not** edit `docs/`, `README.md`, or existing tests.
- Keep `make docs-test` under ~5 minutes; run fences sequentially in one wheel install.
