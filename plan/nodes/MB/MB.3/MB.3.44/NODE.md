---
id: MB.3.44
title: "The deployment-gate seam must not report a proceeding manifest as a refusal"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.43]
paths: [crates/blastit/src/stage0.rs, crates/blastit/tests/critical_php_embed.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T03:31"
updated: "2026-11-15T03:41"
---

# Goal

Close the two behavioural/documentation MINORs from MB.3.43's reviewer that are in
`crates/blastit/src/stage0.rs`. Both concern the deployment-gate seam MB.3.43 added
(`deployment_boot_check`, used by `critical_php_embed`'s call-site witness).

# Context

- `crates/blastit/src/stage0.rs:967` `deployment_boot_check` maps `PhpBoot::Absent`
  to `Err(…)`, while its own doc contract says `Ok(())` means "the gate proceeds" and
  a deployment with no `[route.php]` legitimately boots. A caller can therefore read
  a *proceeding* manifest as a refusal — the exact confusion the witness exists to
  prevent.
- `crates/blastit/src/stage0.rs:1036` the `missing_claim` doc claims "never a broken
  symlink, which resolves to the `ENOENT` that sentence is right about" —
  `std::fs::metadata` follows symlinks, so a broken symlink *is* reported as missing.
  The behaviour is right; the rationale is not.

# Acceptance

```console
$ cargo test -p blastit --locked
$ make check
$ uv run --no-project tools/critical_guard.py --base 304c5b1 --node MB.3.44
```
- `deployment_boot_check` returns `Ok(())` for `PhpBoot::Absent` and `Err` only for a
  refusal, with a test that pins both arms (a manifest with no `[route.php]` proceeds;
  a refused pair still produces §4.1's sentence).
- The `missing_claim` doc states what `fs::metadata` actually does with a broken
  symlink, or drops the parenthetical.
- `critical_php_embed`'s `the_deployment_gate_is_invoked_behind_its_sentence` still
  goes red when `deployment_refusal`'s call site is deleted (paste the run) — the
  witness must survive the seam change.
- `make check` exit 0; no assertion weakened; protected edits authorized by
  `critical_guard --node`.

# Notes

- Do not move the `#[doc(hidden)] pub use` seam (`host.rs:69`) out of `host`: that
  needs `lib.rs`, which is out of scope; it stays the recorded follow-up.
