---
id: MB.3.64
title: "Give the shim-swap boot test a deterministic seam instead of a 20-second race"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit/src/**"
  - crates/blastit/tests/critical_php_embed.rs
  - "crates/blastit-view/**"
  - Cargo.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T21:06"
updated: "2026-11-15T21:06"
---

# Goal

Give `critical_php_embed::a_shim_replaced_between_the_stages_refuses_the_boot` a **deterministic
seam**, so the swap happens between stage 0's record and stage 1's open because the code says so —
not because the runner happened to be slow.

# Context

CI (`smoke (root)` on `71dcaf8`) failed that test at ~:2186: the harness acts on stage 0's
announcement and retries 4 times within 20 s, and on that runner **every attempt served**, so the
"a removed inode pin produces the same served attempts" panic fired. The test's own comment admits
there is no deterministic seam without a `src/` change.

That is now the decision: a protected security test that passes only when the scheduler is slow is
exactly the green this milestone has learned to distrust. `src/` is outside this node's paths — the
node adds them and `critical_guard --node` authorizes the protected edit.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ make rs-test-php-image
$ make check
$ uv run --no-project tools/critical_guard.py --base 71dcaf8 --node MB.3.64
```
- A **seam** makes the window deterministic: after stage 0 has recorded the shim and before stage 1
  opens it, the boot pauses until the test says go (an env-gated point, e.g.
  `BLASTIT_TEST_SWAP_POINT=<path>`, read **in stage 0** and carried in the configuration document —
  the stage 0→1 `execve` strips every `BLASTIT_*`, so an env var cannot cross that barrier by
  itself; `php_probe_timeout_ms` is the worked example). Compiled under a test-only feature or an
  explicitly documented gate, and inert in production.
- The test swaps the shim **while paused**, releases, and asserts the refusal every run: **20/20
  consecutive passes** (paste the loop), and it must still fail when the pin is removed.
- With the seam disabled (unset), the boot behaves exactly as today — no timing dependency added to
  production.
- The test's comment no longer claims a non-deterministic method; the retry loop goes away.
- `make check` exit 0; the protected edit is authorized by `critical_guard --node`; no assertion
  weakened, no `#[ignore]`.

# Notes

- Alternative, only if a seam is genuinely unacceptable: narrow the test to the record comparison it
  can prove deterministically and say plainly in `RESULT.md` that the boot-window swap is
  CI-nondeterministic and therefore not asserted. Prefer the seam.
- `BLASTIT_TEST_*` must never be readable from a route process; keep it stage-0/stage-1 only.
