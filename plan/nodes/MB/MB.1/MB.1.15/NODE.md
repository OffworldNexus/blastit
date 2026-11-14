---
id: MB.1.15
title: "Close the remaining blastit-manifest survivors and protect the patcher's wrong-route case"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-manifest/tests/mutation_gaps.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:44"
updated: "2026-11-13T19:49"
---

# Goal

Close the last meaningful `blastit-manifest` mutation survivors the gate audit adjudicated as real, and put the patcher's two integrity cases (in-string header state, a grant attached to the wrong route) into the protected tier.

# Context

- `docs/testing/AUDIT-MB1.md` (`G4`). Remaining survivors from the whole-crate run (`./mutants.out/`, `MB.1.2`): `parse.rs:197` (`check_route` `&&` → `||`) and `parse.rs:220` (`check_grant` `==` → `!=`), plus `edit.rs:384`/`:411` (`write_atomic`'s two match guards → `true`).
- `parse.rs` is outside `MB.1.2`'s paths and `MB.1.1` only adjudicated `edit.rs`, so these two became follow-ups with no plan task. The model still rejects the same documents later (`UnknownKey`), so the likely observable difference is the **error list/message**, not acceptance — but that must be proved, not assumed. Either pin the exact diagnostic or write the equivalence proof in `RESULT.md`.
- The two `write_atomic` guards are **not** equivalent: with a bare relative name (`BLASTIT_MANIFEST=blastit.toml`), `path.parent()` is `Some("")`, so the `true` mutant ends with `File::open("")` → `ENOENT` **after** the rename succeeded. The callers are `dev_learn.rs`/`check.rs` with `config.manifest`, so the shape is reachable through the environment.
- `crates/blastit-manifest/tests/edit_branches.rs` (MB.1.1) is standard tier. Two of its cases are manifest-integrity oracles: a patcher that loses string state can write into a grant's literal, and a well-formed grant attached to the **wrong route** passes `verify()` (which checks structure, not provenance). Both belong with the protected edit suite.

# Acceptance

```console
# 1. The bare-relative-name branch is pinned hermetically (no process-wide cwd mutation that can race
#    another test; re-exec this test binary into a temp cwd with an env-var guard, or an equally
#    isolated child).
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test edit_branches --locked -- --nocapture
# the new test fails on a /tmp copy with `!parent.as_os_str().is_empty()` → true, and with the
# `AlreadyExists` guard → true; it passes on the shipped tree. Paste both failing probes.

# 2. The two parse.rs survivors are killed or proved equivalent, with the input named:
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test mutation_gaps --locked
# a `[route.groups]` table and an unknown grant key holding a table pin the exact error list (or
# RESULT.md proves no reachable input distinguishes the mutant).

# 3. The patcher integrity cases are protected (this file is in paths; extend, never weaken):
$ grep -c '#\[test\]' crates/blastit-manifest/tests/critical_manifest_edit.rs   # grew by >= 1
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_manifest_edit --locked

# 4. Re-measured, and the adjudicated-real lines are gone from the miss list:
$ make mutants-rust PKG=blastit-manifest     # disk scratch, memory-capped (MB.1.11)
$ test ! -s mutants.out/timeout.txt && echo "0 timeouts"
$ grep -E 'parse\.rs:(197|220)|edit\.rs:(384|411)' mutants.out/missed.txt || echo "adjudicated survivors closed"
# misses <= 11 and killed stays >= 97.0 %; every remaining line has a written equivalence argument.

# 5. Gates.
$ make rs-lint && make rs-doc
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node <ID>
$ uv run --no-project tools/plan.py validate
```

# Notes

- No production behaviour change is expected; if a guard is genuinely unreachable, say so with the reachability argument, not with `#[mutants::skip]`.
- Do not edit any `crates/blastit-manifest/tests/critical_*.rs` except `critical_manifest_edit.rs`, which is listed in `paths` for the two new cases only. No assertion may be weakened or deleted.
- `make check` builds a wheel; the lead runs it at the gate. `make mutants-rust` is one campaign — never run it in parallel with another leaf's campaign (LESSONS: shared `MUTANTS_TMP` kills both).
- Paste into `RESULT.md`: before/after counts, the failing-mutant probe output, and the equivalence arguments for anything still missed.

- `make mutants-rust` uses the disk-scratch/memory-cap wrapper from `MB.1.11`; never point `TMPDIR`
  at `/tmp` on this host (tmpfs) and never share a scratch dir with a concurrent campaign.

- The second guard (`edit.rs:411`, `AlreadyExists` → `true`) needs a different probe: make the
  directory non-writable (`chmod 0555`) so the temp `open` fails `EACCES`; the real code returns
  `EACCES`, the mutant removes a non-existent temp and returns `ENOENT`. Assert `PermissionDenied`
  explicitly. Cargo-mutants runs unprivileged, so this kills there; under uid 0 the permission check
  is bypassed — say so in the doc comment and do not rely on this test under `smoke (root)`.
