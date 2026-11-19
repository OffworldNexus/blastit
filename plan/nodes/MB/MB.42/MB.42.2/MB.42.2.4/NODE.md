---
id: MB.42.2.4
title: "Integration glue: make install staging, test-critical staging, and one-binary four-runtime E2E"
kind: task
status: review
owner: implementer
priority: 1
depends_on: [MB.42.2.3]
paths: [Makefile, "scripts/**", "e2e/**", "crates/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T17:18"
updated: "2026-11-19T22:54"
---

# Goal
Ship the integration seam: `make install` / `make test-critical` stage the universal bundle (binary + LSP + all shims + pure-Python package), and **one** end-to-end run proves a single `blastit` binary serves Python 3.13, Python 3.14, PHP 8.4 and a static mount, with the local handoff working in a real project.

# Exact Deliverables
1. **`make install`**: stage the universal `blastit`, `blastit-lsp`, `libblastit_php*.so`, `libblastit_py*.so` under `$(PREFIX)/share/blastit/lib/` and `python/blastit/*.py` under `$(PREFIX)/share/blastit/python/`, with `<prefix>/bin/{blastit,blastit-lsp}` entry points (design §6.1 layout); `PREFIX` defaults to `~/.local` but **every test invocation overrides it** to a scratch dir. Drop the `-p blastit-dbproxy` build step (design §1.1).
2. **`make test-critical`**: stage `target/debug/blastit` at `$(CRITICAL_STAGE)/blastit/_bin/blastit` and symlink `_bin/blastit-dbproxy -> blastit`, so protected `test_lsp_stdio.py` / `test_check_contract.py` run with zero edits (§3); the stage must be built by the `-p blastit` graph (never `--workspace`).
3. **`fast-check` / `rs-no-python` wiring**: `rs-no-python` assembles the release bundle (`-p blastit`, `--max-glibc 2_28` where the script supports it) and asserts zero `libpython`/zero `RUNPATH`; `fast-check` gains the cheapest new gates (portable-ELF on the debug binary, `cargo test -p blastit-audit`, handoff suite) without re-running the wheel/container matrix.
4. **One E2E, `e2e/universal_binary.py`** (stdlib, `uv run --no-project`), run against the **staged** binary: (a) CPython 3.13 `.venv` WSGI project -> 200 + body; (b) CPython 3.14 `.venv` WSGI project -> 200 + body; (c) PHP 8.4 route -> 200 + body; (d) static mount -> 200 + correct bytes; (e) the same staged binary with a project-local `.venv/bin/blastit` wrapper -> `blastit dev` handoff observed; (f) `blastit check --json` on a 3.12 venv project -> `{"ok":true}` with no per-minor `.so` in the venv. One binary path is used for every leg; assert the file's `sha256` is identical across legs.

# Acceptance
- `make fast-check`
- `make install PREFIX=$(mktemp -d)/prefix` then `find <prefix> -type f -o -type l` matches §6.1 exactly (both entry points, both lib binaries, 2 Python shims, 4 PHP shims (3 minors ok pre-`MB.42.5`), the Python package files), and `<prefix>/bin/blastit --version` works.
- `make test-critical` (protected suites, unedited) and `uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.2.4` clean.
- `uv run --no-project e2e/universal_binary.py --binary <staged> --python 3.13 --python 3.14 --php 8.4 --static --handoff --check` -> all legs green, zero `Mock*Server`.
- `make rs-no-python` -> `check-portable-elf.sh` clean on the bundle's `blastit`.

# Notes
- **1 Cargo writer**; staging/`make install` runs must never write `~/.local` or `/usr/bin` — always an explicit `PREFIX` under `target/`/`$TMPDIR`. Never install or modify the host's PHP/Python packages; PHP legs use the container image.
- Build the shipped binary with `-p blastit` only: a workspace-wide `cargo build` invites feature unification back into `libpython` (`LESSONS.md`).
- No `Mock*Server`/`StubPolicy` fixtures; legs drive real sockets and real daemons. No `scripts/check_*.py` regex guards.
- Do not modify `crates/*/tests/critical_*.rs` or `python/tests/critical/`; out-of-scope finds to `## Follow-ups`.
- If a leg needs PHP 8.5/8.6 the gap belongs to `MB.42.5`, not here.
