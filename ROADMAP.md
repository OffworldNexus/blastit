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
| **B3** dbproxy | `blastit-dbproxy` | A2, A4, pregresql#1 | §2.2 | pregresql hooks: adopted tagged listeners, `SCM_CREDENTIALS` check, per-(route, proxy) upstream, A4 policy, `sql.denied` / `pg.refused` events, per-proxy connection caps; tested against a real PostgreSQL in CI (service container) | ✅ |
| **B4** view | `blastit-view` | A3 | §2.5, §9.3 | embedded CPython (pyo3): frames → WSGI `environ` → one OS thread per request → response frames → `DONE`; `blastit.route` in `environ`; `wsgi.input` streaming; tested with small WSGI apps | ✅ |

## Phase C: init and the real thing

| Step | Crate | Depends on | Architecture | Done when | Status |
|---|---|---|---|---|---|
| **C1** init (C1a `blastit-sys` ✅, C1b binary) | `blastit` (binary) | all of A and B | §2.1, §4, §7.2 | stage 0 / `execve` barrier / stage 1, dbproxy fork + exec with empty env, listener binding and `chown`, privilege drop, single-threaded forks, preload + `gc.freeze`, zygote over `ctl.chan`, log collection (single stdout writer), supervision and graceful shutdown | ✅ |
| **C2** e2e | `e2e/` | C1 | all | Docker-based end-to-end test in CI: a small Django app + PostgreSQL; routing, denials, logs, secret barrier (the spike's R5 scan, as a test) | ✅ |
| **C3** memory | `blastit` | C1 | §8 sizing, §9.2 | heap immortalisation before fork, `BLASTIT_MAX_VIEW_PROCESSES` LRU cap, memory high-watermark retirement | ✅ |
| **C3b** immortalisation on 3.14 | `blastit` | C3 | §9.2 | heap immortalisation skipped (with a warning) on CPython 3.14, where it corrupts a real app's heap; `BLASTIT_IMMORTALIZE=auto\|1\|0` | ✅ |
| **C3c** sharing on 3.14 | `blastit` | C3b | §9.2 | a way to keep the preloaded heap shared on CPython 3.14 that survives its mortal-only decrefs (or a measurement showing what it costs without) | ⬜ |
| **C4** seccomp | `blastit` | C1 | §7.3 | per-role filters, `egress` grants | ✅ |
| **C5** DO | — | C2 | §10, §11 | deploy the e2e app on App Platform; re-run the spike checks against the real binary; measure a real app's memory | ✅ |

## Phase D: adoption

| Step | Where | Depends on | Done when | Status |
|---|---|---|---|---|
| **D1** README | `README.md` | — | a friendly README: the idea (keep the blast radius of a security issue small by sandboxing each view, letting it use private resources without knowing their secrets, and giving it only the privileges it needs over them), then a quick start for a Django and a FastAPI project; every later step of this phase updates it | ✅ |
| **D2** packaging | `pyproject.toml`, `python/`, `crates/blastit-py`, CI | C | installable like granian: a maturin-built pyo3 extension plus a `blastit` command written in Python, so blastit runs on the interpreter of the environment it is installed in and finds its virtualenv for free; dbproxy ships in the wheel as a separate binary without Python; CI builds manylinux wheels for CPython 3.13 and 3.14 on every push to `develop` and publishes them on a GitHub release listed by a PEP 503 index on GitHub Pages; README install section | ✅ |
| **D3a** SQL shapes | `blastit-sqlpolicy`, `blastit-manifest`, `blastit-dbproxy` | — | grants that fit a real Django app: placeholders compared as `$?`, `IN ($?, ...)` of any length and repeated `VALUES` rows match one grant; `unrestricted = true` on a route's own grant (never in a group) for routes such as a staff-only back-office, flagged at boot | ✅ |
| **D3b** pytest plugin | `python/blastit/pytest_plugin.py` | D2, D3a | with pytest-django, records each request a test makes (method, path, the route blastit would pick, the SQL as it goes on the wire) and diffs it against `blastit.toml` with blastit's own routing and SQL matching: unrouted paths, routes never exercised, statements executed but not granted (with the test that ran them), grants never used; text and JSON reports, meant for a human or an agent filling in the manifest; README section | ✅ |
| **D3c** manifest tooling, from the pilot | `python/blastit/pytest_plugin.py`, `blastit` | D3b | statements of global groups are not reported as unused per route; a way to serve a live-server suite (e.g. Playwright against `live_server`) through blastit itself, so real front-end traffic is checked against the manifest; a helper that folds `sql.denied` events from logs into `blastit.toml` (tests are a lower bound: middleware paths such as a 404's redirect lookup, data-dependent query shapes) | ⬜ |
| **D4** pilot | a pilot project | D2, D3b | a real Django project runs under blastit on App Platform, with a manifest written from its test suite through D3b | ✅ |
| **D5** fair queuing | `blastit-scheduler` | D4 | the scheduler's admission queue is fair between clients, so one client's burst can't starve the others (reference: granian's HTTP fair queuing) | ⬜ |
| **D6** rootless mode | `blastit`, `blastit-sys` | C | init runs as an unprivileged user holding only the capabilities it needs (`CAP_SETUID`, `CAP_SETGID`, plus whatever the private directories need, such as `CAP_CHOWN`/`CAP_FOWNER`), from `--user … --cap-add` or file capabilities on the binary, instead of root; the uid layout, the secret barrier and the sandboxes stay the same, and boot says which mode it runs in. Tested end to end on a DigitalOcean Droplet created for the run (ams3) and destroyed afterwards: Docker Compose runs PostgreSQL in one container and the e2e app in another, started with `--user` and only those capabilities, and the e2e scenario and log checks run against it. For platforms other than DigitalOcean App Platform, which can't grant those capabilities to a non-root user (there, starting as root stays the way) | ⬜ |
