---
id: MB.61
title: "Explain why serving is Linux-only (kernel facilities, not packaging)"
kind: task
status: done
owner: writer
priority: 1
depends_on: []
paths: [docs/local.md, docs/index.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-21T14:14"
updated: "2026-11-21T14:19"
---

# Goal

Owner request while reviewing PR #64: the docs say the *server* is Linux-only and the *editor* is not, but never say **why** — a reader can only conclude it is a packaging gap. Explain, in public, that `blastit` serving relies on Linux kernel facilities with no portable equivalent, so a port to other operating systems is a design project rather than a recompile, and that serving is therefore Linux **for now** while the editor stays native everywhere.

Two public places carry the split today:

- `docs/local.md` (lines 18-21, the collapsed `??? note "Developing on macOS or Windows?"`)
- `docs/index.md` (lines 31-38, the `!!! note "Linux, and one file for every interface"`)

## What to write

Put the **one** authoritative explanation in `docs/local.md`'s existing collapsed note (a platform caveat belongs in a collapsed note, never before command 1). Keep the first two sentences as they are (server via Docker/Compose or Linux VM; `blastit-lsp` native on Linux/macOS/Windows, linking `[Editors](#editors)`), then add a short paragraph of *evidence*, not apology:

- it is what the guarantees are built on, not packaging: production's per-`view` isolation is a real uid split (`setresuid`, `CAP_SETUID`/`capset`), static reads are confined by `openat2` (`RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`, with a verified-walk fallback), the outgoing-data refusal is a seccomp filter, and roles identify each other through `SCM_CREDENTIALS` on Unix sockets;
- so a port means replacing those mechanisms (a design project), which is why serving stays Linux **for now**, while the editor ships native everywhere.

These mechanism claims are verified in this tree and must stay true as written: `crates/blastit-sys/src/caps.rs` (`setresuid`/`capset`/`CAP_SETUID`), `crates/blastit-view-static/src/static_asset.rs` (`rustix::fs::openat2` + `ResolveFlags`, `supports_openat2` walk fallback), `crates/blastit-sys/src/seccomp.rs` (seccomp-BPF), `crates/blastit-sys/src/sock.rs` (`ucred`/`SCM_CREDENTIALS`). `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md:36` already states the same four mechanisms internally — do not contradict it.

Then in `docs/index.md` add **one** short clause to the existing sentence that points at the explanation (e.g. "... or a Linux VM, because serving is built on Linux kernel facilities ([why](local.md#))"), so the reader of the front page learns the reason exists without the mechanism list being duplicated.

## Hard constraints

- Do NOT invent a mechanism that is not in the code, and do not claim dev (`--dev`, single uid) has the uid split — that split is what production enforces. Phrase accordingly.
- Do NOT add a roadmap promise, an apologetic tone, a platform matrix, or a diary heading; no `!!! warning` before the first command.
- Keep `docs/local.md` and `docs/index.md` each **<130 lines**.
- One canonical explanation only: `docs/local.md`. No third copy anywhere in `docs/` or `README.md`.
- Leave `ARCHITECTURE.md` and `docs/decisions/` alone.

# Acceptance

- `grep -rn "kernel facilit" docs/` shows the reason on exactly the pages above, with `docs/local.md` the only place the mechanism list appears.
- `make docs-check` green (strict build, nav/orphan check, page length).
- `wc -l docs/local.md docs/index.md` both < 130.
- `uv run --no-project tools/guard.py` clean, `uv run --no-project tools/plan.py validate` OK.
