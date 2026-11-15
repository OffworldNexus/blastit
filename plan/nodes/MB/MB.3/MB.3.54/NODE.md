---
id: MB.3.54
title: "MB.3.53 review MINORs: two assertions that cannot fail, the unwitnessed stage-0 call site, and a page that contradicts the new gate"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.53]
paths:
  - crates/blastit/tests/critical_raw_ingress.rs
  - crates/blastit/src/stage0.rs
  - docs/frameworks/index.md
  - docs/ci.md
  - docs/troubleshooting.md
  - "e2e/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T08:32"
updated: "2026-11-15T08:44"
---

# Goal

Close the four MINOR findings from MB.3.53's reviewer. Two are assertions that cannot
fail, one is an unwitnessed call site, and one is a page that now contradicts the gate this
branch just added.

# Context

MB.3.53 (commit `86f885b`) made `body = "raw"` without `allow_raw = true` a boot refusal in
production. Its reviewer PASSed and left:

1. `crates/blastit/tests/critical_raw_ingress.rs:297,334,381` — `!listening(port)` is
   asserted **after** the child exited, so a boot that bound and *then* exited (exactly the
   mutation state the suite exists to catch) also satisfies it. The assertion cannot fail.
   Assert the absence of a `ready` event in all three cases (stage 1 emits `ready` only
   after `bind_tcp`), or probe the port while the boot is in flight.
2. `:272-292` — the JSON case asserts the descriptor (stdout, stderr empty) but never the
   `"stream":"stderr"` field, so flipping the field would pass. Assert
   `event["stream"] == "stderr"` on the parsed event.
3. `crates/blastit/src/stage0.rs:1904` — no unprivileged suite calls `stage0::run`, so
   deleting that call site (and its position before `bind_view_sockets`) leaves every test
   green; only a manual root/container run witnesses it. Land the `e2e/` "boot refusals"
   scenario already listed as MB.3.53's follow-up 4, or add the smallest seam that asserts
   the gate runs before the bind. Note: the reviewer's judgement is that this cannot reopen
   the fail-open — production stage 1 re-reads the manifest and refuses there, and that half
   IS process-tested — so this is coverage honesty, not a hole in the gate.
4. `docs/frameworks/index.md:68-69` still says `raw` "the boot warns either way" and
   presents `allow_raw` only as what stops the `check` finding, which contradicts the new
   production refusal. Add the boot-refusal clause. (`docs/ci.md:30` and
   `docs/troubleshooting.md:37` are the same class and are already on MB.3.53's follow-up
   list; take them here if their paths allow, otherwise list them.)

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_raw_ingress
$ make check
$ make docs-check
```
- Each of the three assertions can FAIL: prove it by mutating the thing it guards (a boot
  that binds then exits; a flipped `stream` field) and pasting the red, then restore with
  `git diff --stat` unchanged.
- The stage-0 call site has a witness that runs in CI, or `RESULT.md` states in one sentence
  what still only a root/container run proves and why that is acceptable.
- `docs/frameworks/index.md` agrees with `docs/reference/manifest.md` and `ADR-0011` about
  what happens at boot without the key; `make docs-check` green.
- `make check` exit 0; no assertion weakened elsewhere; no `#[ignore]`.

# Notes

- Item 1 must not become a `sleep`: the ordered signal is the `ready` event (absent) or the
  child's exit, and `MB.3.48` already established how to wait for a boot's streams.
- `crates/blastit/tests/critical_raw_ingress.rs` is a protected file and this node's paths
  name it: add/strengthen only.
