# Log MB.56.3

## 2026-11-20T16:02 [conductor]

created (task) Consolidate crates/blastit/tests/ (30+ files -> 4 crisp user-story E2E suites)

## 2026-11-20T16:33 [lead]

MUST DO: delete the temporary #[doc(hidden)] pub use check as audit_v1; shim in crates/blastit/src/lib.rs + check::learn::plan_learn_json (dead prod code kept only for 3 crates/blastit/tests suites), when consolidating into the 4 e2e suites

## 2026-11-20T17:28 [lead]

MB.56.2.1 inventory: crates/blastit-scheduler/tests/static_feature_off.rs stays (needs --no-default-features; keep as its own leg). Cases to re-prove E2E: per-view grant isolation, fair queueing+debt TTL, exit()/fatal DONE, static ../symlink/dotfile/.php matrix, live reload, PHP pinning+rewritten entry, CPU accounting, X-Forwarded-For trust, both openat2+Walk backends (StaticRoot::open_with)

## 2026-11-20T17:28 [lead]

MB.56.2.1 REVIEW-GAP (must re-cover E2E): (1) blastit check exit table 0 clean/1 findings/2 could-not-start/3 suite-failed incl pytest 5 = 3, docs/reference/cli.md:78-80; (2) pytest-django handshake: settings module/manage.py/test_<db>, owner-only never-reused handshake file removed however run ends, Django without postgres proxy + store with no inputs = exit 2 naming the block; (3) check --learn human line 'learn: N routes, M queries added -> /abs/Blastfile' and 'proposed (dry-run)' never 'added'/relative, to stderr under --json; (4) check finds its gate root from installed/repacked bundle (never a cargo tree), prefers project venv interpreter, prepends caller PYTHONPATH

## 2026-11-20T17:41 [lead]

MUST COVER in e2e_prod_refusals/e2e_dev (from MB.56.2.2 review): (a) env boot refusals naming the variable never its value: missing $IDENT, DATABASE_URL with target_session_attrs=any, multi-host URL, output naming BLASTIT_*, two proxies producing one variable (all one line each in one boot); BLASTIT_* stripped from worker env; injected default never overrides a present value; (b) WSGI host refusals verbatim: 'blastit dev: CPython 3.12 at <path> is not supported for WSGI views (CPython 3.13 and 3.14 ...)', free-threaded refusal, missing libpython sentence naming the minor and every remedy

## 2026-11-20T18:33 [lead]

MUST PORT before deleting crates/blastit/tests/critical_lsp.rs: (1) multi-error document -> multiple diagnostics (warning_sites_are_their_declarations / diagnostics_are_capped); (2) didClose publishes [] with no version; (3) formatting a BROKEN buffer answers null not an error; (4) non-BMP UTF-16 position (positions_are_utf16); (5) single-quoted query '\''... $1'\'' quick fix + nothing offered outside queries + context_diagnostics never trusted. MB.56.2.5 ports these.

## 2026-11-20T18:44 [lead]

status ready -> active

## 2026-11-20T18:44 [lead]

attempt 1/3 started

## 2026-11-20T18:44 [lead]

base d313856b4918afad5267c015b2ad87ed561b578a

## 2026-11-20T18:45 [lead]

edited fields

## 2026-11-20T21:22 [lead]

status active -> done: four BDD suites + common/; 26,643 -> 11,513 LOC
