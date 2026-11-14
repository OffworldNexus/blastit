---
id: MB.15
title: "Prove and pin: blastit dev reaps its tree on SIGTERM (the MB.14 orphan was an artifact)"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.14]
paths: ["crates/blastit/tests/**", docs/knowledge/LESSONS.md, Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T13:58"
updated: "2026-11-14T14:43"
---

# Goal

**Proof + pin** (this node was opened as a bug fix and the bug does not exist): prove with a transcript that `blastit dev` reaps its whole process tree when the dev init PID is signalled, pin that behaviour with a cheap regression test, record the tooling trap that produced the false report, and confirm the Makefile needs no workaround (`MB.14` landed on a plain `kill -TERM` after the false report was corrected, so there is nothing to remove).

# Context

`MB.14` first concluded that a `SIGTERM` to the `blastit dev` PID orphaned stage 1 and left the port answering, and its earliest draft stopped the served stack with `setsid` + a process-group kill. That draft never landed: the false report was withdrawn mid-node and `MB.14` committed `9fab605` with a plain `kill -TERM "$pid"` + bounded wait + pid-anchored leftover scan.

The premise could not be reproduced against a freshly built `target/debug/blastit`:

- `blastit dev` **`execve`s in place** into `--stage=1` (`crates/blastit/src/stage0.rs`, `exec_stage1_dev`): the PID a caller starts is the same process as init, and its command line no longer contains `dev`.
- `crate::supervise` already implements `SIGTERM`/`SIGINT` → `DRAIN`, wait or `BLASTIT_GRACEFUL_TIMEOUT`, `SIGTERM`, `SIGKILL` (`crates/blastit/src/supervise.rs`).
- `kill -TERM <init pid>` logged `shutdown drain` → `process.exited … signal=15` → `shutdown stopped`, the port refused connections, and no `--dev-root=/run/user/<uid>/blastit-dev-*` process was left.

The false report had two causes: the probe's `$!` was a shell wrapper rather than the server, and `pgrep -f "blastit dev --port N"` can never match the process after the in-place `execve`, so a surviving-looking tree was actually never signalled.

The behaviour is real today and untested; that is the gap this node closes.

# Acceptance

1. **Formal reproduction, pasted verbatim** against a binary built from the checked-out sources (`$(RUST_ENV) $(CARGO) build -p blastit --locked`):
   - start the server with a plain background launch (no `setsid`), record the PID, and show `ps -o pid,pgid,cmd -p "$PID"` proving it is the same PID now running `--stage=1` (the in-place `execve`),
   - show `GET /` answering `200`,
   - send `SIGTERM` to that PID **only**, then show within the grace window: the process is gone, `curl` fails, and `pgrep -f -- "--dev-root=.*-$PID"` is empty,
   - a second transcript for `SIGINT`, and one for a second `SIGTERM` during the drain.
   Then state in the result, in one paragraph, that the original observation was an artifact of (a) a wrapper losing `$!` and (b) the in-place `execve` defeating a `pgrep` on the launch command line.
2. **Regression test** in `crates/blastit/tests/` (standard tier unless it belongs to an existing protected suite's contract — then protected, and if a protected suite would have to change, stop and report that as its own node): start `blastit dev` on a free port with a tiny static manifest, wait for the port, send `SIGTERM` to the init PID, and assert (a) the listening socket refuses within `BLASTIT_GRACEFUL_TIMEOUT` plus slack, (b) the process exits within the same bound, (c) no child of that tree survived, (d) the `--dev-root` directory is gone. No `sudo`, no fixed port, bounded runtime, polling with a deadline (no sleeps-as-assertions).
3. **`docs/knowledge/LESSONS.md`** gains exactly this trap, one bullet: "`blastit dev` execve's in place into stage 1 — the PID is stable but the command line is not, so `pkill -f 'blastit dev …'` finds nothing after boot; signal the PID you started, and do not wrap the launch in a way that loses it."
4. **Makefile** (`depends_on MB.14`): confirm `make docs-serve-check` already tears the server down with a plain `kill "$pid"` plus a bounded wait (SIGKILL only as a last resort) — no `setsid`, no process-group kill — and that the "no process left behind" assertion still holds (`git show 9fab605:Makefile`). Change it only if the transcript or the test shows it is needed; state which you chose and why. This proves the CLI is usable by ordinary callers.
5. **If formal reproduction finds a real failure window** — a signal arriving before the handler is installed, a race with `execve`, a wedged second signal during drain — then this is a bug again: report the exact window and state, with evidence, and do not close it as an artifact.

# Notes

- The promise under test: after `blastit dev` exits, nothing of it is still listening.
- Do not paper over a CLI defect in the Makefile, and do not edit protected suites without an authorizing node.
