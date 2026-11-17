# Result MB.23.5

## Summary

`blastit dev` reloads what changed. One `fanotify` group feeds a union of the preload phase's and
every worker's reported files; the debounce is 50 ms quiet / 250 ms hard max / 500 ms global floor
on an injected clock; one fire retires every view together with `Ctl::Reload` (`0x45`, no route
payload), so no two views can serve different versions of a shared module. A `Blastfile` or
`preload` save re-parses, validates and re-execs while the `:8080` listener and the proxies stay
up; an invalid intermediate save logs one `Blastfile:<line>:<col>` and the previous topology keeps
serving. `--no-reload` installs nothing at all. `dev.reload` and `dev.reload.unavailable` are the
only new events, and one `manifest.warning` names any grant swap the dbproxy could not apply.

Descendants, all `done`: MB.23.5.1 (`54ad5d9`), MB.23.5.2 (sub-epic, `fde43da`), MB.23.5.2.1
(`83c6bb9`), MB.23.5.2.2 (`bdd23c5`), MB.23.5.3 (`81e603f`).

## Delivered, per leaf

**MB.23.5.1 — the wire and the events** (`54ad5d9`). The framing a reload rides on: the
`Ctl::Reload` consume path and the two events `dev.reload` and `dev.reload.unavailable`, so one
fire is observable as one message and one event.

**MB.23.5.2.1 — the policy** (`83c6bb9`). The debounce state machine and the union with class
precedence, the supervisor's single `Source` set and armed deadline folded into its existing poll,
real `--no-reload`, the `reload: on (N directories)` boot line, and the 14-leg live-reload suite.

**MB.23.5.2.2 — the cycle** (`bdd23c5`). Validate-then-`execve`, the adoption document that
carries the inherited listener and marks across the exec, the cycle and its abandon path, the
three test seams, and five more live legs.

**MB.23.5.3 — the grant refresh** (`81e603f`). The dbproxy swaps its allowlist atomically behind
an `Arc` on a reload instead of reconnecting, PHP views report their loaded files, and the PHP
reload row joins the suite.

## Gate evidence

| Command | Result |
|---|---|
| `cargo test -p blastit --test critical_dev_reload` | `23 passed` (repeated idle and under all cores busy, and 3x with `--features python,php`) |
| `cargo test -p blastit --lib` | `289 passed, 2 pre-existing ignored` |
| `cargo test -p blastit-dbproxy` | `81 + 5 + 9 + 24 + 1 passed` |
| `cargo test -p blastit --test critical_php_embed` | `27 passed` |
| `make check` | `exit 0, 0 FAIL, 0 FAILED` |
| `make docs-test` | `48 fences, 30 executed, 0 failed, dev-flags EXECUTED` |
| `make seam-check` | `3 seams present in debug, 3 absent from release` |

## Open follow-ups

- The `policy_out_of_step` taint is not carried across the exec: a single silent save in a narrow window, self-healing; the fix is an adoption-document field, filed to MB.23.6.
- Acceptance 6's dbproxy `200` needs a job with `BLASTIT_TEST_PG_URL` set.
- The listener/store-set warning is per-save by design once a cycle has happened.

## Open unknown

One `critical_dev_reload` run failed early during MB.23.5.2.2 attempt 3 and its panic was destroyed by piping through `tail -2`; it never reproduced in 100+ subsequent runs, including many with
every core busy, and it is recorded as unexplained rather than blamed on host load. Future failures must be captured to a full log file.
