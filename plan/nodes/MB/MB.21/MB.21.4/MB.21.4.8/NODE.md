---
id: MB.21.4.8
title: "Bound the dev test client's response pairing: parse a response against its own request, close after a body, and deadline every wait with a small diagnostic"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.4.4]
paths: [crates/blastit/tests/critical_dev_and_fail_closed.rs, docs/knowledge/LESSONS.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T14:44"
updated: "2026-11-16T16:28"
---

# Goal

`dev_boots_five_unprivileged_roles_and_strips_secrets` stops being flaky at gate rate. The suite's own
HTTP client (`Dev::request`, `crates/blastit/tests/critical_dev_and_fail_closed.rs:442`) must read the
response that belongs to the request it just wrote: parse a head plus its declared body and never a buffer
it cannot prove complete, close the connection whenever it sent a body, and bound every wait so a hung or
half-read pair fails fast with a small diagnostic (method/target, the bytes read, the server's last stdout
lines) instead of passing a stale status to the assertion.

# Context

- **Measured defect:** the protected assertion `assert_eq!(rejected.status, 413)` saw `200` for `POST /env`
  in 2 of the last 4 full `make check` runs on this host, and in one reviewer run — while the captured
  `blastit dev` log of the same run printed `POST /env → home 413 unmatched`. It has never reproduced in
  21 targeted re-runs. So the failure is not a refusal difference: it is the wrong response being read, or
  the right response being read incompletely, and only under full-gate load.
- **The helper** (verified by `grep -rn "fn request" crates/blastit/tests/`: only `smoke.rs:315` and this
  file — nothing in `tests/common/`): opens a fresh `TcpStream`, sets a 20 s read timeout, writes head and
  body, then `read_to_end(..).unwrap_or_default()` — which **swallows** the timeout/reset error while
  keeping whatever bytes arrived — and `Response::parse` (`:562`) takes the *first* status line of the
  buffer and everything after the first `\r\n\r\n`. That is three untested assumptions: one response per
  connection, the read completed, and the bytes read belong to this request.
- **Body-carrying requests are the ones that break it:** `POST /env` (expects `413`) and `POST /json`
  (expects `400`) are answered at the head, before the body is consumed, so the body is still in flight
  when the server has already decided and may reset/close. `dev.request` writes the body with
  `write_all(..).unwrap()` and then reads to EOF: a reset in either direction lands in exactly the
  swallow-and-parse path above.
- **Prior art:** `docs/knowledge/LESSONS.md`'s `MB.2.5.2` entry (the scheduler-hang paragraph): a hang in a
  waiter suite is usually a pairing nobody released plus an unbounded spin, and the fix is a bounded budget
  in the low thousands that panics with a small summary, never with every frame.
- Hypotheses, cheapest first; the leaf must decide which one it is and say so in `RESULT.md`:
  1. **client framing** — the read timed out or was reset mid-response, `unwrap_or_default()` hid it, and
     `Response::parse` accepted a partial buffer (or the wrong head, if anything else was ever on that
     socket);
  2. **pairing/port** — a body-carrying connection was answered by a process that is not the `blastit dev`
     under test: the suite's cases run in parallel in one binary and `free_port()` cannot lease its port to
     the child, so a sibling that is still shutting down (or a retry attempt that already exited) can answer
     the first bytes; `wait_for_ready` only proves *something* accepts;
  3. **server-side** — dev http logs its `413` decision and writes the refusal onto a connection or response
     that belongs to another request (stale view frame, wrong fd). This leaf's paths stop at the test file:
     if this is the mechanism, keep the reproduction, name the file and line, and open the follow-up (Notes).

# Acceptance

```console
# make's own embedded-CPython environment, without hardcoding this host's prefix
$ eval "$(make -s --eval='print-rust-env: ; @echo export PYTHONHOME=$(PYO3_PREFIX) PYO3_PYTHON=$(PYO3_BIN) LD_LIBRARY_PATH=$(PYO3_LIBDIR)' print-rust-env)"
$ BASE=$(git rev-parse HEAD)     # capture before the first edit; record it in RESULT.md
$ nproc; uptime
$ for c in $(seq 1 $(( $(nproc) / 2 ))); do yes > /dev/null & done
$ for i in $(seq 1 20); do \
    cargo test -p blastit --features python --locked --test critical_dev_and_fail_closed \
      -- --exact dev_boots_five_unprivileged_roles_and_strips_secrets --nocapture \
      || { echo "run $i failed"; break; }; \
  done
$ pkill -x yes
$ for i in 1 2 3; do make test-critical || break; done
$ make critical-skip-check
$ make rs-lint
$ uv run --no-project tools/critical_guard.py --base "$BASE" --node MB.21.4.8
$ git diff -U0 -- crates/blastit/tests/critical_dev_and_fail_closed.rs
```

1. **20/20** runs of that one test pass under load (the loop completes without `break`; paste the tail and
   the `nproc`/`uptime` that describe the load). The two body-carrying cases are the ones this exercises.
2. **`make test-critical` exit 0 three times consecutively** (Rust and Python critical tier, zero skips).
3. **The protected assertion is intact**: the `git diff` above touches no `assert_eq!(rejected.status, 413)`
   line and no expectation is relaxed, retried, `#[ignore]`d or skipped; `make critical-skip-check` is
   clean; `critical_guard.py --node MB.21.4.8` reports no protected write outside this leaf's declaration.
   `make rs-lint` (`clippy --all-targets -D warnings`) is clean.
4. The helper now (a) parses a response head, then exactly its declared body (`Content-Length`, or
   read-to-close), and refuses to parse a buffer it cannot prove complete; (b) closes the connection
   whenever it sent a body, and never reuses one; (c) has a bounded deadline on the whole exchange and
   panics with method/target, the bounded bytes read and the last stdout lines — not with a full frame dump.
5. `RESULT.md` **states the mechanism** in one paragraph: which hypothesis was true, with its evidence
   (reproduction rate, bytes read, log lines), plus the fix and, for anything server-side, the file/line and
   the follow-up leaf.

# Notes

- Only `crates/blastit/tests/critical_dev_and_fail_closed.rs` and `docs/knowledge/LESSONS.md`.
  `crates/blastit/tests/common/**` is deliberately **not** in `paths` (the helper is in this file, see the
  grep above). `smoke.rs:315` has its own twin client: do not edit it; if the same defect is proven there,
  record it as a follow-up.
- **No product code.** `crates/blastit/src/**` is out of scope, and `Cargo.toml`/`Cargo.lock` belong to
  `MB.21.4.5` alone — hand-parse with `std`, no HTTP-client dependency. If the mechanism turns out to be
  server-side (hypothesis 3), do not paper over it: keep a deterministic reproduction
  (a hand-rolled `TcpListener` in a `#[test]` is enough), write the file/line into `RESULT.md` and open the
  follow-up leaf naming it. The flake is then *not* claimed closed, and the report says so.
- **Never weaken the test.** No `#[ignore]`, no retry-until-413, no relaxing to `>= 400`, no widened
  timeout as the *fix*. A timeout is a diagnostic, never the assertion. `tools/critical_guard.py` protects
  this file; `make critical-skip-check` refuses a skip construct anywhere in it.
- Prefer a deterministic pin beside the timing evidence: one `#[test]` that drives the helper against a
  local listener which answers a body-carrying request early and splits/resets the response, asserting the
  helper either reads the true head or panics with its diagnostic — never passes a stale one. That is what
  keeps this fixed after the load-dependent repro stops being available.
- The suite's cases run in parallel in one binary; do not serialize tests to hide it (horrible under load
  and it hides the pairing). If port reuse is the mechanism, prove *which* process answered (`/proc` pid vs
  `dev.child.id()`) before changing anything.
- Add one `LESSONS.md` line only if the mechanism is general and non-obvious (e.g. "a body-carrying request
  answered at the head must not be followed by a read-to-EOF parse"); do not duplicate the `MB.2.5.2` entry.
