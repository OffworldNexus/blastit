---
id: MB.56.2.2
title: "Delete non-§7 micro-tests in php-sapi, sys, env, ingress, py-shim, events, frame, py-sapi, lsp/src"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.2.1]
paths:
  - "crates/blastit-php-sapi/**"
  - "crates/blastit-sys/**"
  - "crates/blastit-env/**"
  - "crates/blastit-ingress/**"
  - "crates/blastit-py-shim/**"
  - "crates/blastit-events/**"
  - "crates/blastit-frame/**"
  - "crates/blastit-py-sapi/**"
  - "crates/blastit-lsp/src/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T17:15"
updated: "2026-11-20T17:15"
---

# Goal
Delete every non-§7 test in the remaining crates: `blastit-php-sapi` (1,782 test LOC), `blastit-sys` (1,816), `blastit-env` (882), `blastit-ingress` (818), `blastit-py-shim` (217), `blastit-events` (92), `blastit-frame`, `blastit-py-sapi`, and the inline `#[cfg(test)]` modules in `crates/blastit-lsp/src/**`.

1. Remove the `tests/**` trees and inline `#[cfg(test)]` modules of those crates.
2. Two **narrow exceptions only if the case is genuinely unreachable from any E2E run against the real binary**: `blastit-sys` may keep **≤ 250 LOC** of malformed-ELF / wire-boundary assertions and `blastit-frame` **≤ 200 LOC** of corrupt/truncated-frame assertions. Justify each in `RESULT.md`; if the case is reachable end-to-end, delete it and record it for `MB.56.3`.
3. Record in `RESULT.md` every unique user-visible case the deleted suites covered (PHP SAPI pinning/`php2shell`, env-var precedence, ingress decode refusals, frame resync) so `MB.56.3`/`MB.56.4` re-cover them.
4. No `#[ignore]`, no `skip`, no production logic change.

# Acceptance
- `make fast-check` green, zero warnings; only `crates/blastit-sys` and/or `crates/blastit-frame` may still contain `#[cfg(test)]`, each within its LOC cap.
