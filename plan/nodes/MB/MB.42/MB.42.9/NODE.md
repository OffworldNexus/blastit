---
id: MB.42.9
title: "Handoff: an unsafe $VIRTUAL_ENV must be skipped, not exit 1 (blasts every dev|check|fmt|lsp in a 0775 checkout)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.2.3]
paths: [crates/blastit/src/cli/handoff.rs, crates/blastit/tests/critical_handoff.rs, Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T23:26"
updated: "2026-11-19T23:35"
---

# Goal
Stop the safe local handoff from **hard-failing** when the venue is merely *ambient*: `$VIRTUAL_ENV` is not an explicit request for a binary, so an unsafe active venv must be **skipped with a warning**, not `exit 1`.

# Why (reproduced, on this repo)
`crates/blastit/src/cli/handoff.rs` treats `$VIRTUAL_ENV/bin/blastit` as an "explicitly named" local install and therefore **exits 1** when it fails a check. On this checkout:

```
$ VIRTUAL_ENV=$PWD/.venv ./target/debug/blastit fmt --check Blastfile
blastit fmt: refusing local handoff to …/.venv/bin/blastit:
  /home/remy/offworld-nexus/blastit is mode 0775, so another user could replace the binary;
  set BLASTIT_NO_LOCAL_HANDOFF=1 to use the global blastit            (exit 1)
```

`/home/remy/offworld-nexus/blastit` is `0775`, so with the venv activated **every** `blastit dev|check|fmt|lsp` in this repo exits 1 — and the same happens in any Docker/devcontainer image built with `umask 002` (dirs `0775`). `make test-critical` already had to export `BLASTIT_NO_LOCAL_HANDOFF=1` to work around it. A user gets a security-flavoured refusal for a `umask` they did not choose, and the tool appears broken.

# Exact Deliverables
1. **Reclassify `$VIRTUAL_ENV/bin/blastit` as auto-discovered**, alongside `./.venv/bin/blastit`, `./vendor/bin/blastit`, `./node_modules/.bin/blastit`: when it fails a §4 check (mode/uid/regular/exec), print the one calm skip line, **keep probing** the remaining candidates, and fall back to the global binary. `exit 1` is reserved for an **explicitly named** binary — i.e. a `BLASTIT_*` override (`BLASTIT_BIN` and friends), where the user really did point at a path.
2. Keep the **order** unchanged (`$VIRTUAL_ENV/bin/blastit` before `./.venv/bin/blastit`), and keep the skip line naming the path, the actual reason, and that probing continued (per `MB.42.2.3`'s wording).
3. Update the §4 tests in `crates/blastit/tests/critical_handoff.rs`: the case currently named for "an unsafe **activated venv** is refused with the fix" becomes "an unsafe activated venv is **skipped** and the next candidate (or the global binary) runs" — assert the skip line on stderr, assert the marker of the *next* candidate ran, assert exit 0. Keep a case for the `BLASTIT_*`-explicit `exit 1` so that path stays covered.
4. Remove the `BLASTIT_NO_LOCAL_HANDOFF=1` workaround from `Makefile`'s `test-critical` **only if** the tier passes without it; if it is still needed for another reason, say why in `RESULT.md`.

# Notes
- This **amends design §4** ("`exit 1` is reserved for an explicitly named local install (`$VIRTUAL_ENV`, `BLASTIT_*`)"). Record in `RESULT.md` that `docs/design/universal-distribution.md` §4 and `docs/design/…`'s "skip vs exit" sentence need a one-line amendment (`$VIRTUAL_ENV` is ambient, not explicit) for the lead to route to `architect`/`MB.42.4` — do **not** edit `docs/**` here.
- Do not weaken any check: the unsafe binary is still never executed; only the *reaction* (skip vs exit) changes.
- Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- The reproduction above now prints the skip line and exits **0**, running the next candidate or the global binary
- `cargo test -p blastit --locked --test critical_handoff` (all cases, updated as above)
- `VIRTUAL_ENV=$PWD/.venv ./target/debug/blastit fmt --check Blastfile` in this repo exits 0 (or performs the format check) instead of refusing
- `make test-critical` green **without** the `BLASTIT_NO_LOCAL_HANDOFF=1` export (or the reason it is still needed)
- `cargo clippy -p blastit --all-targets --locked -- -D warnings` and `--no-default-features`
- `make fast-check`
