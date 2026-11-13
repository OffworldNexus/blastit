---
id: MA.9
title: "Leaf 16: un-hang the smoke (root) CI job and make it fail loudly"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [.github/workflows/ci.yml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T11:52"
updated: "2026-11-13T12:08"
---

# Goal

Make the `smoke (root)` CI job pass reliably, or fail loudly and fast — it currently **hangs indefinitely** instead of running.

# Context

Release PR #62 (branch `release/0.1.1`). MA.7 rewrote this job's step because the runner image moved to **sudo-rs**, which ignores `sudo -E`:

```yaml
- run: sudo -E env "PATH=$PATH" cargo test -p blastit --locked -- --test-threads=1
```
became
```yaml
- run: sudo env "PATH=$HOME/.cargo/bin:$PATH" "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" "PYO3_PYTHON=$PYO3_PYTHON" "BLASTIT_TEST_PG_URL=$BLASTIT_TEST_PG_URL" cargo test -p blastit --locked -- --test-threads=1
```

Result: on run 43407063422 the job sits in that step for **35+ minutes** with no log output (`BlobNotFound` for the in-flight log), while the same tests took ~5 s on `develop` at `2c05139` under classic `sudo -E`. GitHub's 360-minute timeout is the only thing that would end it. MA.7's local simulation (`env -i HOME=/root` → `cargo 1.100.0`) does not reproduce it, so the difference is runner-specific.

Candidate causes to distinguish, in order:
1. **sudo-rs waiting on a password prompt** because the NOPASSWD rule is not matched the way classic sudo matched it — it would block on `/dev/tty` forever and produce no log.
2. **root building from scratch** into the runner-owned `target/` (ownership/`--locked` interaction), i.e. a very long build rather than a hang.
3. **A test as root blocking on something the environment no longer provides** (a socket path, a uid it cannot create, a database connection with a retry loop) because some env var classic `-E` used to pass is now missing.

# Acceptance

1. The step becomes **self-diagnosing and bounded**:
   - `timeout-minutes` on the job (e.g. 15–20) so a future hang fails loudly instead of burning the 360-minute ceiling;
   - a cheap preflight that fails fast with a real error: `sudo --non-interactive true` (or `sudo -n id`) plus `sudo --non-interactive env ... cargo --version`, each echoing what it resolved;
   - the test step itself uses `--non-interactive` (never a password prompt) and streams output so an in-flight log exists (e.g. `--nocapture` for the smoke tests, and/or `| tee` so partial output is visible).
2. Root cause is named with evidence, not inferred: state which of the three candidates it was and what proves it (e.g. `sudo -n` succeeding/failing, cargo's build plan, a test's blocking syscall). If CI evidence is the only way to settle it, say so explicitly and make the next run produce that evidence.
3. `cargo test -p blastit` still runs **as root** — the job's purpose is unchanged.
4. `make check` exit 0 and `uv run --no-project tools/plan.py validate` ok; diff limited to `.github/workflows/ci.yml` (plus a test file only if a root-blocking test genuinely needs a guard, which must be justified).
5. Report exactly what the next CI run must show for this job to be considered green.

# Notes

- Do not "fix" this by dropping `sudo` or by skipping tests: the root smoke test is the only CI proof that the privileged path boots.
- Do not weaken any guard or add `continue-on-error`.
- Do not push; the conductor pushes and watches the run.
- If you decide a different invocation is fundamentally more robust (e.g. build as the runner user with `--no-run`, then execute the test binaries as root explicitly), justify it and keep the same coverage.
