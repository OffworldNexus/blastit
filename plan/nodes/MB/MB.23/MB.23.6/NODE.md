---
id: MB.23.6
title: "Verify MB.23 live: runnable-docs --no-reload fence, zero-inotify guard on standalone + wheel, production strace, all-worker reload determinism and dev_reload mutation audit"
kind: task
status: ready
owner: "test-curator"
priority: 1
depends_on: [MB.23.5]
paths: ["docs/testing/AUDIT-MB.23.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T23:13"
updated: "2026-11-16T23:21"
---

# Goal

Prove `MB.23` the way a user experiences it and the way a regression would hide, then write
the audit:

- execute the live documentation verbatim (`make docs-test`), including the
  `$ blastit dev --no-reload` fence, and record the verdict line;
- re-run the ban guards on the **standalone** binary and on the **wheel**'s `_blastit*.so`;
- prove production never watches anything (`strace` transcript on `blastit check` and on a
  production-mode boot: zero `fanotify_init`/`fanotify_mark`), and that
  `crates/blastit-sys/src/seccomp.rs` is byte-identical;
- audit the reload tests for determinism (no sleep-decided verdicts) and mutation-test the
  policy module;
- prove that one save retires **every** view worker together while `:8080` keeps
  accepting, and that no test still expects a per-route/surgical restart;
- write `docs/testing/AUDIT-MB.23.md` and file every open gap as a follow-up node.

# Context

- The audit is a `test-curator` document, not a second design: findings cite
  `file:line`, state the impact in the user's words, and carry a severity
  (`BLOCKER`/`MAJOR`/`MINOR`) like the other `AUDIT-*.md` files.
- The reload policy is the security-relevant half (the loaded-file union, self-trigger
  exclusion, the all-worker transaction, `Blastfile`/`preload` survival, and the listener
  staying open) and is the module that must clear the **75 %** mutation floor; the raw
  syscall bodies in `blastit-sys` are excluded from the floor (they need a real kernel) but
  their pure helpers — mask encoding, errno mapping, handle keying — are in scope.
- Audit the tests for the **dropped** behaviour too: no test may assert that only one view
  restarted, that an untouched route's pid stayed the same, or that a per-route map exists.
  A surviving "surgical reload" assertion is a MAJOR finding, not a style nit.
- Guards to re-run, and what a pass looks like:
  - `scripts/check-no-inotify.sh` on `target/debug/blastit` **and** on the wheel extension
    produced by `make docs-test` (`target/docs-wheel/**/blastit/_blastit*.so`), each printing
    one `ok` line.
  - `cargo tree -p blastit -e normal --locked | grep -Ei '\b(notify|inotify)\b'` → empty.
  - `grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/` (public pages only) → empty.
- `make docs-test` is the only gate that runs the `--no-reload` fence; if it passes, the
  documented command boots, prints the banner naming its port, and stops on `SIGTERM`.
- `MB.23.5`'s `RESULT.md` is the evidence source for the live-reload rows; this task
  re-runs the suites rather than trusting it.

# Acceptance

```console
$ make docs-check
$ make docs-test
$ make test-critical
$ make docs-unit
$ scripts/check-no-inotify.sh target/debug/blastit
$ scripts/check-no-inotify.sh $(ls target/docs-wheel/*/blastit/_blastit*.so | head -1)
$ cargo tree -p blastit -e normal --locked | grep -Ei '\b(notify|inotify)\b'   # no output
$ grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/local.md docs/reference/ docs/troubleshooting.md   # no output
$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/check.strace target/debug/blastit check -- ./scripts/noop 2>/dev/null || true; ! grep -q fanotify /tmp/opencode/check.strace
$ grep -n "sleep(" crates/blastit/tests/critical_dev_reload.rs
$ cargo mutants --in-place --file crates/blastit/src/dev_reload.rs -- --locked
$ git diff --quiet -- crates/blastit-sys/src/seccomp.rs Cargo.toml Cargo.lock
$ uv run --no-project tools/plan.py validate
```

1. `make docs-test` executes every fence including the reload flag, and its report shows
   `executed=True` for that fence.
2. Both `check-no-inotify.sh` runs print `ok`; the dependency grep and the public-docs grep
   are empty; `seccomp.rs`, `Cargo.toml` and `Cargo.lock` are unchanged across the whole
   epic (`git diff --quiet`).
3. The `strace` transcript for `blastit check` (and a production-mode boot on the same
   fixture) contains no `fanotify_init`/`fanotify_mark` line.
4. `dev_reload.rs` kills **≥ 75 %** of mutants, and every surviving mutant is either
   justified in the audit with a sentence or filed as a follow-up node.
5. `grep -n "sleep("` finds only bounded startup polls with a deadline and a comment (no
   fixed sleep deciding a verdict) — or nothing at all.
6. `docs/testing/AUDIT-MB.23.md` exists: severity counts, the evidence above with command
   tails, the gate host's fanotify result (`reload: on (N directories)`), proof that one
   save reloads every view together with `:8080` held open, and every open gap as a
   follow-up node id.

# Notes

- If `make docs-test` cannot run because another gate holds the documented ports, wait —
  the harness's lock exists precisely for this; do not run a second gate in parallel.
- If a guard fails, file the finding and stop; the fix is a new child of `MB.23`, not an
  edit from this task.
- Record the wheel-extension path you actually checked; do not report a guard you did not
  run against a real file.
