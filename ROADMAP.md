# Roadmap

How blastit gets built from [`ARCHITECTURE.md`](ARCHITECTURE.md). Every step is
one pull request into `develop`, from a `feature/<step>-<slug>` branch, with
green CI. Steps in the same batch are independent and can be built in
parallel; a batch starts once the batches it depends on are merged.

Each step lists the crate it owns, the architecture sections it implements,
and what "done" means. Library steps come first, so that the privileged,
process-level code at the end is mostly wiring of tested parts.

| Status | Meaning |
|---|---|
| ⬜ | not started |
| 🟦 | in progress (PR open) |
| ✅ | merged |

## CI, step by step

CI grows with the code, never ahead of it:

* every PR: fmt, clippy, rustdoc, tests, MSRV (Python 3.13 for the pyo3 crate);
* from B3: a PostgreSQL service container for dbproxy's tests;
* from C2: the Docker end-to-end test (`e2e/`): a Django app with log in,
  notes and log out, served by blastit, with denials and the secret barrier
  checked in the logs;
* from C5: the same app live on DigitalOcean App Platform (ams3), with a fresh
  managed PostgreSQL per run, deleted afterwards.

## Phase A: pure libraries (no privileges, no processes)

### Batch A1 (parallel)

| Step | Crate | Architecture | Done when | Status |
|---|---|---|---|---|
| **A1** manifest | `blastit-manifest` | §5.1 (proxy *declarations*), §6 | TOML model for `version`, top-level `groups`, `[proxy.*]` (protocol, input map, output map, kept as raw strings), `[group.*.<proxy>]`, `[[route]]` (name, path, methods, groups, egress, prefork, inline grants); boot validation (unknown group/proxy, duplicate names, bad regex, grants on unknown proxy, unattached group → warning); effective grants per (route, proxy); uid table; routing table (`match(method, path_info) → route`), full-match, first wins | ✅ |
| **A2** events | `blastit-events` | §8.1 | event model with common fields; JSON encoder (one line, ≤ 2048 bytes, truncation listed in `truncated`); `app.output` splitting into `part`/`parts`; pretty encoder (coloured, one line); never-logged fields cannot be expressed by the API | ✅ |
| **A3** frame | `blastit-frame` | §3, §3.1 | `u32 len \| u8 type \| u32 id \| payload` codec with max frame size; typed `REQ_HEAD/BODY/END`, `CANCEL`, `RESP_HEAD/BODY/END/ABORT`, `DONE`; `ctl.chan` messages; streaming decoder that handles partial reads; fuzz-style round-trip tests | ✅ |
| **A4** sqlpolicy | `blastit-sqlpolicy` | §5.2 (grant contents), §6 (matching) | normaliser (trim, trailing `;`, whitespace collapse outside quotes/identifiers); allowlist built from a grant set; `transaction_control` (BEGIN/COMMIT/ROLLBACK/SAVEPOINT/RELEASE/ROLLBACK TO, whole statements only); multi-statement strings never match | ✅ |

### Batch A2 (after A1)

| Step | Crate | Architecture | Done when | Status |
|---|---|---|---|---|
| **A5** env | `blastit-env` | §5 | template engine (`{{ f }}`, `{{ f \| urlencode }}`, errors on unknown field/filter); `postgresql` protocol: input slots (`url` *or* split, `ca_cert`), upstream config parsing, template fields, default outputs; app environment = platform env − inputs − `BLASTIT_*` − `BLASTIT_ENV_STRIP` + outputs + injected defaults; collision rules; secrets never reachable from templates or errors | ✅ |

## Phase B: the processes, as libraries with in-process tests

| Step | Crate | Depends on | Architecture | Done when | Status |
|---|---|---|---|---|---|
| **B1** http | `blastit-http` | A2, A3 | §2.3 | HTTP/1.1 front end over an inherited listener and a frame channel; keep-alive, chunked bodies, `100-continue`, pipelining in order, limits and timeouts, header validation, `Upgrade` → 501, `http.rejected` events; tested against a fake scheduler over a socketpair | ✅ |
| **B2** scheduler | `blastit-scheduler` | A1, A2, A3 | §2.4 | routing via A1, `404` when unmatched, global/per-route budgets counted in threads (`DONE`), bounded queue, timeouts (`504`, slot held), stuck-thread recycling, lazy spawn / prefork / idle retirement / LRU cap over `ctl.chan`, health path, `request` events; tested with fake http and fake views | ✅ |
| **B3** dbproxy | `blastit-dbproxy` | A2, A4, pregresql#1 | §2.2 | pregresql hooks: adopted tagged listeners, `SCM_CREDENTIALS` check, per-(route, proxy) upstream, A4 policy, `sql.denied` / `pg.refused` events, per-proxy connection caps; tested against a real PostgreSQL in CI (service container) | ⬜ |
| **B4** view | `blastit-view` | A3 | §2.5, §9.3 | embedded CPython (pyo3): frames → WSGI `environ` → one OS thread per request → response frames → `DONE`; `blastit.route` in `environ`; `wsgi.input` streaming; tested with small WSGI apps | ✅ |

## Phase C: init and the real thing

| Step | Crate | Depends on | Architecture | Done when | Status |
|---|---|---|---|---|---|
| **C1** init (C1a `blastit-sys` ✅, C1b binary) | `blastit` (binary) | all of A and B | §2.1, §4, §7.2 | stage 0 / `execve` barrier / stage 1, dbproxy fork + exec with empty env, listener binding and `chown`, privilege drop, single-threaded forks, preload + `gc.freeze`, zygote over `ctl.chan`, log collection (single stdout writer), supervision and graceful shutdown | ✅ |
| **C2** e2e | `e2e/` | C1 | all | Docker-based end-to-end test in CI: a small Django app + PostgreSQL; routing, denials, logs, secret barrier (the spike's R5 scan, as a test) | ✅ |
| **C3** memory | `blastit` | C1 | §8 sizing, §9.2 | heap immortalisation before fork, `BLASTIT_MAX_VIEW_PROCESSES` LRU cap, memory high-watermark retirement | ⬜ |
| **C4** seccomp | `blastit` | C1 | §7.3 | per-role filters, `egress` grants | ⬜ |
| **C5** DO | — | C2 | §10, §11 | deploy the e2e app on App Platform; re-run the spike checks against the real binary; measure a real app's memory | ✅ |
