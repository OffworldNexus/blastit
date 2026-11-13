---
id: MB.1.1
title: "Bound blastit-manifest cursor loops and close edit.rs mutation survivors"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T15:36"
---

# Goal

Make every scanning loop in `crates/blastit-manifest` structurally progress-bounded so a mutated cursor cannot spin, and close the `edit.rs` survivors (`verify()` at `edit.rs:961`, the TOML comment/bracket patcher branches), so the whole-crate run in `MB.1.2` stops paying 90 s timeout slots and clears 75 %.

# Context

- Baseline (`plan/nodes/MA/RESULT.md` §2, `cargo-mutants v27.1.0`): whole crate `437/630` viable (69.4 %); `edit.rs` `133/320` (42.9 %) with **177 missed** and **10 timeouts**; `parse.rs:252` (`position`) one more timeout. Policy modules (`validate.rs`, `routing.rs`, `parse.rs`, `model.rs`) are 95.0 % — **do not churn them**.
- Every timeout is a byte-cursor arithmetic mutation (`+=` -> `*=`, `-=` -> `/=`) that stops advancing: `edit.rs` `render`, `walk`, `parse_header`, `closing_bracket`, `parse.rs` `position`. Fix the loop **shape** (consume from an explicit iterator/`get(i)`, or a `debug_assert!`/step guard that proves the cursor moved each iteration) — never raise `MUTANTS_TIMEOUT`. A loop that cannot be proved to advance is the bug.
- `crates/blastit-manifest/tests/critical_*.rs` are protected and are **not** in this node's `paths`: add new integration coverage in the new non-critical `crates/blastit-manifest/tests/edit_branches.rs` (a new file is always allowed). Never weaken or delete an existing assertion anywhere.
- `edit.rs` writes manifests that are always re-parsed by `Manifest::parse_and_validate` before the atomic write: keep that invariant; do not delete `verify()` — cover it.

# Acceptance

```console
# 1. Crate tests green, including the new bounded-loop / patcher-branch tests.
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
# exit 0; the new tests terminate (no #[ignore], no sleep, no timeout):
$ grep -c 'fn ' crates/blastit-manifest/tests/edit_branches.rs    # >= 6

# 2. Free disk first (a full /tmp shows up as a linker SIGBUS, LESSONS.md).
$ df -h /tmp | tail -1

# 3. The file-scoped run this leaf is measured by (327 mutants, one run only).
$ mkdir -p /tmp/opencode/blastit-mutants
$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-manifest -f '**/src/edit.rs' -j 2 --timeout 90 -o /tmp/opencode/mb1-manifest-edit
$ test ! -s /tmp/opencode/mb1-manifest-edit/mutants.out/timeout.txt && echo "0 timeouts"
$ awk '{n[$NF]++} END {v=n["caught"]+n["missed"]+n["timeout"]; printf "caught %d / viable %d = %.1f%%\n", n["caught"], v, 100*n["caught"]/v}' \
    /tmp/opencode/mb1-manifest-edit/mutants.out/outcomes.txt
# 0 timeouts, and the printed file ratio >= 60.0 % (baseline 42.9 %).

# 4. The verify() backstop is no longer a blind spot.
$ grep -i 'verify' /tmp/opencode/mb1-manifest-edit/mutants.out/missed.txt || echo "no missed mutant in verify()"

# 5. Nothing else broke.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- Paste the 4 output tails and the exact timeout/miss counts into `RESULT.md`. The crate-wide 75 % number belongs to `MB.1.2` (this leaf's successor) — report the file ratio here.
- `mutants.out` layout: `outcomes.txt` one line per mutant (last field is `caught`/`missed`/`timeout`/`unviable`), `missed.txt`, `timeout.txt`.
- Loop-bounding is the security-relevant part: an unbounded scan is also a denial-of-service on a hostile manifest. Keep the byte budget/overflow checks.
- Do not edit `crates/blastit-manifest/src/{validate,model,routing,uid,error}.rs` (other leaves own them) and do not add dependencies.
