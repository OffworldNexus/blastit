# RESULT — MB.2.6: wire static routes into `blastit dev` and `blastit check`

Status: **done** (base `cb7a1e4`, branch `release/0.2.0`).

## Summary

A manifest whose every route is static now boots under `blastit dev` and is
audited by `blastit check` with **no `app`**; a mixed tree without `app` keeps
the old `BLASTIT_APP` refusal under `blastit`/`blastit dev` (`check` never
demanded one — its suite supplies the callable), and a `BLASTIT_APP` that no
route could reach is refused by name everywhere. A static root that is missing,
a file, unlistable or unreadable by the responder is named with the manifest and
the route before
anything is written to stdout, in `dev`, production and `check` — and a readable
root under an owned `0700` parent (the ordinary `$HOME/app` layout) is
accepted, because `dev`/`check` ask the kernel in-process rather than through a
dropped-to probe. `check` on an all-static manifest audits every root, runs no
pytest, no dbproxy and no observation file, and exits `0` — or `2` when a root is
refused — with the v1 `--json` document unchanged. In a mixed tree a static route
the suite never reached is one stderr line, not a finding.

No manifest key, CLI flag, environment variable, finding kind, `fix.action` or
`--json` field was added.

## What changed (by path)

| File | Change |
|---|---|
| `crates/blastit/src/config.rs` | `Config::manifest_dir` (absolute manifest directory), set once in `from_env_with_format` via the new `manifest_dir()` and carried in the stage-1 document (`to_json`/`from_json`). |
| `crates/blastit/src/stage0.rs` | Production and `dev` boot order is now read → `parse_and_validate` → **`app_rule`** (§ below) → `check_static_roots` → `static_root_permission` → env swap/listeners. `MB.1.6`'s "file missing before `BLASTIT_APP`" survives. |
| `crates/blastit/src/stage1.rs` | `load` returns `py.None()` and skips the app import, `check_view_paths`, preload, warmup, `gc.collect`/`freeze`/immortalise when `config.app.is_none()`; both `dev` route sites chain `.with_static(route.static_policy().cloned(), manifest.app().is_some())`; `scheduler_config` sets `manifest_dir` and `static_max_bytes = STATIC_MAX_FILE_BYTES`. |
| `crates/blastit/src/check.rs` | `static_app_refusal` (only the all-static half, exit `2`), `check_static_roots` + the same visibility probe, then the all-static branch: `check --learn` → `2` (`nothing to learn: every route is static`); otherwise `--json` prints `Report::all_static` and the human form prints nothing, exit `0`. Mixed trees print `  route "<name>": static root "<root>" was never requested` on stderr after the audit. `check` still needs **no** `app` (§ attempt 3). |
| `crates/blastit/src/audit_v1.rs` | `Report::all_static(&manifest)` (the v1 shape with zero counts, empty `proxies`/`findings`), `unexercised_routes` skips a route with a `[route.static]` policy, and new `pub fn unrequested_static_roots(manifest, table, observations)`. No new `Finding`, no new `Fix`. |
| `crates/blastit/tests/critical_static_cli.rs` | **New protected suite**, 12 cases (below). |
| `crates/blastit/src/cli.rs` | **Unchanged** — see Deviations. |

### The app rule, split by owner (`stage0.rs`)

```rust
/// Half (a): a `BLASTIT_APP` no route of an all-static manifest could reach.
pub(crate) fn static_app_refusal(config: &Config, manifest: &Manifest) -> Option<String>

/// A **boot** owns both halves.
pub(crate) fn app_rule(config: &Config, manifest: &Manifest) -> Result<(), String> {
    if let Some(message) = static_app_refusal(config, manifest) { return Err(message); }
    if !manifest.is_all_static() && config.app.is_none() { return Err("BLASTIT_APP: … is required"); }
    Ok(())
}
```

`blastit check` takes **only** half (a) — it never demanded an `app`, because
the suite's own configuration supplies the WSGI callable (attempt 3, below).

`static_root_permission` asks the kernel **in-process** when `probe == geteuid()`
(`dev`, `check`), and only uses the dropped-to `visibility::unreadable` probe
for a genuinely different responder uid (production's `UID_BASE+2`).

## Exit-code / behaviour matrix

| # | Setup | Command | Exit | Observable |
|---|---|---|---|---|
| 1 | all-static, `app` absent, `public/index.html` | `blastit dev` | runs | `GET /` → `200` + index bytes; `GET /dashboard/settings` → `200` (spa); banner `… · 1 route`; no `BLASTIT_APP` |
| 2 | static + forwarding route, no `app`, **no `public/`** | `blastit dev` | `1` | stdout empty; stderr `BLASTIT_APP: the WSGI callable (module:attribute) is required`; **no** `static root` (app rule precedes root audit) |
| 3 | all-static, `public/` missing | `blastit dev` | `1` | stdout empty; stderr `blastit.toml`, `route "site"`, `static root "public"`, `no such file or directory` |
| 4 | all-static, root present | `blastit check --json` | `0` | one v1 document, `ok true`, `findings []`, `requests/statements 0`, `routes {total 1, exercised 0}`, `proxies {}`; no pytest (no `PATH`, so a suite attempt would be exit `2`) |
| 5 | all-static, root present | `blastit check` | `0` | **nothing on stdout** |
| 6 | all-static, `public/` missing | `blastit check [--json]` | `2` | stdout empty; stderr names `blastit.toml`, `route "site"` and `static root "public"` |
| 7 | static + forwarding route, no `app` | `blastit check` | `2` (root) / suite path | **no** `BLASTIT_APP` demand: the mixed fixture has no `public/`, so it stops on the root; with the root present it reaches the pytest preflight (attempt 3 pinned) |
| 8 | all-static, root present | `blastit check --learn` | `2` | stderr `nothing to learn: every route is static` |
| 9 | app + app route + static route, one request to the app route | in-process `audit_v1::evaluate` | — | `ok true`, no `UnexercisedRoute` for the static route, `unrequested_static_roots → [("site","public")]`; without `[route.static]` the same route **is** an `UnexercisedRoute` (negative twin) |
| 10 | project under an owned `0700` parent, `public/` 0755 | `blastit check --json` **and** `blastit dev` | `0` / runs | one clean document; `GET /` + spa deep path `200` (the ordinary `$HOME/app` local case) |
| 11 | `chmod 000 public` | `blastit check --json` / `blastit dev` | `2` / `1` | stderr `route "site"`, `static root "public"`, `permission denied`; stdout empty |
| 12 | `public` is a regular file | `blastit check --json` / `blastit dev` | `2` / `1` | stderr `route "site"`, `static root "public"`, `not a directory`; stdout empty |
| 13 | all-static + `BLASTIT_APP=demo:application` | `blastit check --json` / `blastit dev` | `2` / `1` | stderr `BLASTIT_APP: every route is static, so no route can reach an app: …`; the app's value never appears |

Rows 1–13 are `critical_static_cli.rs`.

Test names: `an_all_static_site_boots_under_dev_with_no_app`,
`a_mixed_tree_without_an_app_still_names_blastit_app`,
`a_missing_static_root_is_named_with_its_route_before_any_output`,
`an_all_static_check_audits_the_root_and_runs_no_pytest`,
`an_all_static_check_exits_two_on_a_missing_root`,
`check_without_an_app_still_runs_the_suite`,
`an_all_static_check_refuses_learn`,
`a_readable_root_under_an_owned_0700_ancestor_is_accepted`,
`a_root_the_caller_cannot_list_is_refused`,
`a_root_that_is_a_file_is_refused`,
`a_blastit_app_on_an_all_static_manifest_is_refused`,
`a_static_root_the_suite_never_requested_is_a_warning_not_a_finding`.

### The suite fails without the code (mutation proof, both reverts applied with `edit`, then restored)

```console
$ # mutation A: `run_dev` back to `if config.app.is_none()`
test result: FAILED. 6 passed; 2 failed
  a_missing_static_root_is_named_with_its_route_before_any_output
  an_all_static_site_boots_under_dev_with_no_app
  stderr: blastit dev: BLASTIT_APP: the WSGI callable (`module:attribute`) is required

$ # mutation B: `check::run`'s `Err` arm returns CLEAN instead of START_FAILURE
test result: FAILED. 6 passed; 6 failed
  left: Some(0) right: Some(2)
  an_all_static_check_exits_two_on_a_missing_root
  an_all_static_check_refuses_learn
  a_blastit_app_on_an_all_static_manifest_is_refused
  a_root_the_caller_cannot_list_is_refused
  a_root_that_is_a_file_is_refused
  check_without_an_app_still_runs_the_suite

$ # mutation C (attempt 2): the in-process branch disabled (`if false && probe == current_uid()`)
test result: FAILED. 11 passed; 1 failed
  a_readable_root_under_an_owned_0700_ancestor_is_accepted
  assertion `left == right` failed: … stderr: "blastit check: static root \"public\": permission denied\n"
```

## Evidence

Every command was run from `/home/remy/offworld-nexus/blastit` with
`PYO3_PYTHON`/`PYTHONHOME`/`LD_LIBRARY_PATH` exported exactly as `Makefile`'s
`RUST_ENV` does (and `CARGO_NET_OFFLINE=true`).

### `cargo test -p blastit --test critical_static_cli --locked`

```console
test a_blastit_app_on_an_all_static_manifest_is_refused ... ok
test a_missing_static_root_is_named_with_its_route_before_any_output ... ok
test a_mixed_tree_without_an_app_still_names_blastit_app ... ok
test a_readable_root_under_an_owned_0700_ancestor_is_accepted ... ok
test a_root_the_caller_cannot_list_is_refused ... ok
test a_root_that_is_a_file_is_refused ... ok
test a_static_root_the_suite_never_requested_is_a_warning_not_a_finding ... ok
test an_all_static_check_audits_the_root_and_runs_no_pytest ... ok
test an_all_static_check_exits_two_on_a_missing_root ... ok
test an_all_static_check_refuses_learn ... ok
test an_all_static_site_boots_under_dev_with_no_app ... ok
test check_without_an_app_still_runs_the_suite ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

### `cargo test -p blastit --locked` (whole crate, untouched suites included)

```console
Running unittests src/lib.rs
test result: ok. 178 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.30s
Running tests/audit_group_hint.rs
test result: ok. 4 passed; 0 failed
Running tests/critical_check_engine.rs
test result: ok. 19 passed; 0 failed
Running tests/critical_config_and_presets.rs
test result: ok. 12 passed; 0 failed
Running tests/critical_dev_and_fail_closed.rs
test result: ok. 19 passed; 0 failed
Running tests/critical_static_cli.rs
test result: ok. 12 passed; 0 failed
Running tests/smoke.rs
test result: ok. 31 passed; 0 failed
```

### `cargo test -p blastit --test critical_config_and_presets --locked` / `--test critical_check_engine --locked`

```console
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

### `cargo test -p blastit-py --locked` (and `--test critical_check_schema --locked`)

```console
Running unittests src/lib.rs (_blastit)
test result: ok. 59 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.36s
Running tests/critical_check_schema.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

### `cargo clippy -p blastit --all-targets --locked -- -D warnings`

```console
    Checking blastit-env v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-env)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.96s
```

### `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` (`make rs-doc`)

```console
    Documenting blastit v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit)
    Documenting blastit-py v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.12s
   Generated …/target/doc/blastit/index.html and 12 other files
```

### `cargo fmt --all -- --check`

```console
$ cargo fmt --all -- --check; echo "fmt_exit=$?"
fmt_exit=0
```

### `uv run --no-project tools/critical_guard.py --base cb7a1e4 --node MB.2.6`

```console
critical-guard: cb7a1e4..working tree, 31 changed file(s)
node MB.2.6: paths ['crates/blastit/src/cli.rs', 'crates/blastit/src/config.rs', 'crates/blastit/src/stage0.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/src/check.rs', 'crates/blastit/src/audit_v1.rs', 'crates/blastit/tests/critical_static_cli.rs']
  no change to a protected critical test
```

Same result through `make critical-guard BASE=cb7a1e4 NODE=MB.2.6`.

### End-to-end fixture (the page's CI one-liner)

```console
$ cat blastit.toml   # all-static, no app
version = 1

[[route]]
name = "site"
path = ['.*']

[route.static]
root = "public"
spa = true

$ ./target/debug/blastit check --json
{
  "schema_version": 1,
  "ok": true,
  "requests": 0,
  "statements": 0,
  "routes": {
    "total": 1,
    "exercised": 0
  },
  "proxies": {},
  "findings": []
}
exit=0

$ ./target/debug/blastit check        # prints nothing
exit=0

$ rmdir public && ./target/debug/blastit check --json
blastit check: ./blastit.toml: manifest has 1 error(s)
  - route "site" (index 0): static root "public": no such file or directory
exit=2
```

## Files

Changed (5) + added (1), all inside the node's `paths`:

* `crates/blastit/src/config.rs`
* `crates/blastit/src/stage0.rs`
* `crates/blastit/src/stage1.rs`
* `crates/blastit/src/check.rs`
* `crates/blastit/src/audit_v1.rs`
* `crates/blastit/tests/critical_static_cli.rs` (new, 740 lines)

No file outside `paths` was touched (`git diff --numstat`: 299 added / 9
removed in `src`, plus the new suite).

## Deviations (named)

1. **The `dev` banner does not report static roots.** `NODE.md` §Context asks
   for "the `blastit dev` banner reports static routes and their roots (no new
   flags)", but the accepted `docs/design/static-and-spa.md` §1.2 settles the
   opposite in writing: the banner stays the three documented lines and
   `1 route`, because the page (normative) shows that exact banner for a
   manifest with no app and `MB.2.7` replays it verbatim; "MB.2.6's note … is a
   Follow-up for the lead; the page wins". `cli.rs` and `dev_learn.rs` are
   therefore untouched (`dev_learn.rs` is not in `paths` anyway) and a static
   route is counted as the route it is. Follow-up 1.
2. **`make check` was not run**, per the dispatch brief ("Do NOT run `make
   check`/`make e2e`"), which supersedes the node's acceptance list. Its Rust
   half is covered above (`cargo test -p blastit`, `cargo test -p blastit-py`,
   clippy, `make rs-doc`, `cargo fmt --check`, `make critical-guard`); the
   Python, docs and plan halves belong to the gate run.
3. **`make critical-guard BASE=develop NODE=MB.2.6` reports 4 false violations**
   because `develop` predates `MB.2.x`'s own protected-test changes
   (`critical_check_engine.rs`, `critical_config_and_presets.rs`,
   `python/tests/critical/test_check_contract.py`). Against this node's dispatch
   base (`cb7a1e4`) the same tool reports "no change to a protected critical
   test" — every acceptance command above used `cb7a1e4`, as the brief names.
4. **Changed-line budget.** 299 production lines + 740 test lines (1027) exceeds
   the ≤600 target. The production side is 9 changed hunks; the suite is one
   harness (fixture, `env_clear` runner, `dev` boot/stop, HTTP GET) plus twelve
   cases with their assertions. Trimming it would drop a leg of the exit-code
   matrix or one of the design's root negatives (attempt 2 added four, including
   the one that pins the `$HOME`-is-0700 bug).

## Follow-ups

1. **Banner root list (design conflict, lead's call).** `NODE.md` wanted the
   `dev` banner to list static routes and their roots; `docs/design/static-and-spa.md`
   §1.2 forbids it and says the page wins. If the lead wants that line after
   all, it is `crates/blastit/src/dev_learn.rs::banner` (not in this node's
   paths) plus a page edit, and `MB.2.7`'s verbatim replay of the banner.
2. **`BLASTIT_APP` on an all-static manifest — decided (attempt 2).** The
   reviewer's preferred option was taken: such a setting is **refused** with a
   named message (`BLASTIT_APP: every route is static, so no route can reach an
   app: unset BLASTIT_APP, or declare a route without [route.static]`), in
   `blastit`, `blastit dev` and `blastit check`. Nothing is silently dropped, and
   `is_all_static`'s "the boot decides" now has one answer. The remaining
   scheduler-side shape (`static_forward` is `manifest.app().is_some()`, design
   §4.6) is consistent with that refusal, because an app that reaches this point
   is always declared in the manifest.
3. **`docs/reference/cli.md` / `docs/troubleshooting.md`.** They describe
   `check` as running a suite; neither is in `paths`. The new text is: an
   all-static manifest audits every `[route.static] root` instead of running
   pytest and exits `0` (`2` for a missing/non-directory root), `--json` keeps
   the v1 shape, `check --learn` is exit `2`, and `BLASTIT_APP` on such a
   manifest is refused. The doc-writer owns them.
4. **Production-only uid probe is unexercised by an unprivileged suite.** The
   `R13` step-5 probe now takes two shapes: in-process `read_dir` when
   `probe == geteuid()` (`dev`, `check` — the leg the suite pins both ways), and
   the dropped-to `visibility::unreadable` probe for a genuinely different
   responder uid. That second leg needs root/a dropped-to child, so
   `StaticRoot::open` in the scheduler's `run` remains the tested-in-deployment
   authority.
5. **Shared-checkout note (not a code issue).** `docs/knowledge/LESSONS.md` was
   already modified by another agent when this node started, and a bare
   `rustfmt` had reordered imports in `crates/blastit/src/*` against
   `style_edition = 2024` (the existing LESSONS bullet on `rustfmt --edition
   2024` covers the cause). `cargo fmt -p blastit` restored the style; no new
   lesson was added, and no out-of-`paths` file was touched.

## Review fixups (attempt 2)

### 1 [MAJOR] `dev`/`check` refused an owned `0700` parent (the headline local flow)

**Cause.** `R13` step 5 used only the dropped-to probe. When `probe == euid`
(`dev`, `check`) the probe child cannot `setgroups(2)`, so
`visibility::unreadable` answered `Spoken::NoProbe` and fell back to
`blocks()` — a mode-bit reading that asks whether a **foreign** uid could pass.
An ordinary `$HOME` (0700, owned by the caller) then read as a refusal.

**Fix** (`crates/blastit/src/stage0.rs`). `static_root_permission` now has two
shapes: when `probe == current_uid()` it asks the kernel **in-process** with the
same `read_dir` the responder's `O_PATH|O_DIRECTORY` open performs (new
`listable()`), and only a genuinely different responder uid (production's
`UID_BASE+2`) goes through the dropped-to `visibility::unreadable` probe. A
genuinely unlistable root is still refused — by `check_static_roots`' `read_dir`
first, and by `listable()` in the same message.

**Repro, before the fix** (this checkout, `target/debug/blastit` built at
attempt 1):

```console
$ mkdir -p /tmp/opencode/mb226-repro/home/app/public
$ chmod 0700 /tmp/opencode/mb226-repro/home      # owned by remy, like $HOME
$ chmod 0755 /tmp/opencode/mb226-repro/home/app/public
$ cd /tmp/opencode/mb226-repro/home/app && ./blastit check --json
blastit check: static root "public": permission denied
Exit code: 2

$ chmod 711 /tmp/opencode/mb226-repro/home && ./blastit check --json
{ "schema_version": 1, "ok": true, … }        # exit 0 — only the 0700 parent changed
```

**After the fix** (same `0700` parent, rebuilt binary):

```console
$ stat -c '%a %U %n' /tmp/opencode/mb226-repro/home
700 remy /tmp/opencode/mb226-repro/home
$ cd /tmp/opencode/mb226-repro/home/app && ./blastit check --json
{ "schema_version": 1, "ok": true, "requests": 0, "statements": 0,
  "routes": { "total": 1, "exercised": 0 }, "proxies": {}, "findings": [] }
exit=0
$ ./blastit check            # the page's human one-liner, prints nothing
exit=0
$ ./blastit dev --port 18099 &   # the headline local flow
  blastit dev · single-uid simulation · 127.0.0.1:18099
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
$ curl -o /dev/null -w '%{http_code}\n' http://127.0.0.1:18099/          # 200
$ curl -o /dev/null -w '%{http_code}\n' http://127.0.0.1:18099/deep/path # 200
```

**And the refusal still holds** for a root nobody can read:

```console
$ chmod 000 public && ./blastit check --json
blastit check: ./blastit.toml: manifest has 1 error(s)
  - route "site" (index 0): static root "public": permission denied
exit=2
$ ./blastit dev
blastit dev: ./blastit.toml: manifest has 1 error(s)
  - route "site" (index 0): static root "public": permission denied
dev_exit=1
```

### 2 [MAJOR] the §7.2 root negatives were missing

Three new cases in `crates/blastit/tests/critical_static_cli.rs`:

| Case | Setup | Assertion |
|---|---|---|
| `a_readable_root_under_an_owned_0700_ancestor_is_accepted` | `<fixture>/home` 0700 → `home/app` with `public/` 0755 | `check --json` exit `0`, empty stderr; `dev` serves `GET /` and the spa fallback |
| `a_root_the_caller_cannot_list_is_refused` | `chmod 000 public` (restored before asserting) | `check --json` exit `2`, stdout empty, stderr `route "site"` + `static root "public"` + `permission denied`; `dev` exit `1` naming the root |
| `a_root_that_is_a_file_is_refused` | `public` is a regular file | `check --json` exit `2` with `not a directory`; `dev` exit `1`, stdout empty |

Mutation proof: with the in-process branch disabled
(`if false && probe == current_uid()`), the first case fails with exactly the
reviewer's sentence — `test result: FAILED. 11 passed; 1 failed` — and passes
again once restored. The 0700 fixture is no longer hidden by `/tmp`'s 1777: the
parent is `home/`, owned by the caller, `0700`.

### 3 [MINOR] the `check` missing-root message now names the route

`an_all_static_check_exits_two_on_a_missing_root` asserts the design's
path + route + root triple (`blastit.toml`, `route "site"`,
`static root "public"`), matching the `dev` twin.

### 4 [MINOR] the three-line banner is asserted in full

`an_all_static_site_boots_under_dev_with_no_app` now asserts all three
documented lines, including the port: `  blastit dev · single-uid simulation ·
127.0.0.1:<port>`, `  Not a sandbox. Production \`blastit\` refuses to run like
this.`, `  http · scheduler · dbproxy · 1 route`. `dev_serves` returns the port
it served on so the first line is exact.

### 5 [MINOR] `BLASTIT_APP` on an all-static manifest is decided

`NODE.md`'s "the boot decides" is now one answer: **a named refusal**, in
`blastit`, `dev` and `check`, through `stage0::static_app_refusal` (the half a
boot and `check` share; attempt 3 split the boot-only half out). The message
names the variable and never its value:

```console
$ BLASTIT_APP=demo:application ./blastit check --json
blastit check: BLASTIT_APP: every route is static, so no route can reach an app: unset BLASTIT_APP, or declare a route without [route.static]
check_exit=2
$ BLASTIT_APP=demo:application ./blastit dev
blastit dev: BLASTIT_APP: every route is static, so no route can reach an app: …
dev_exit=1
```

Pinned by `a_blastit_app_on_an_all_static_manifest_is_refused` (exit `1`/`2`,
stdout empty, and the app's value `demo:application` absent from stderr).

### Re-run after the fixups

```console
$ cargo test -p blastit --locked
test result: ok. 178 passed; 0 failed; 1 ignored   (lib)
test result: ok. 4 passed   (audit_group_hint)
test result: ok. 19 passed  (critical_check_engine)
test result: ok. 12 passed  (critical_config_and_presets)
test result: ok. 19 passed  (critical_dev_and_fail_closed)
test result: ok. 12 passed  (critical_static_cli)
test result: ok. 31 passed  (smoke)

$ cargo test -p blastit-py --locked
test result: ok. 59 passed; 0 failed               (_blastit)
test result: ok. 6 passed                          (critical_check_schema)

$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ cargo fmt --all -- --check; echo "fmt_exit=$?"
fmt_exit=0

$ uv run --no-project tools/critical_guard.py --base cb7a1e4 --node MB.2.6
critical-guard: cb7a1e4..working tree, 31 changed file(s)
  no change to a protected critical test
```

## Gate fixup (attempt 3) — `check` must not demand an `app`

### The regression

Attempt 2's split was in the wrong place: `blastit check` called the **boot**
rule, so its second half — `BLASTIT_APP: the WSGI callable
(module:attribute) is required` — fired for every non-all-static manifest that
does not declare an `app`. `blastit check` never required one: the suite's own
pytest configuration supplies the WSGI callable, and this node added no reason
to start demanding one.

The gate caught it as **18 failures** in `python/tests/critical`, all in
`test_check_contract` (e.g. `test_a_clean_run_is_one_document_and_no_findings`,
`test_a_corrupt_observation_line_exits_two_with_an_empty_stdout`,
`test_a_dead_control_channel_exits_three`,
`test_a_denied_statement_exits_one_with_one_document`), plus my own
`check_without_an_app_on_a_mixed_tree_still_names_blastit_app` — a test that
encoded the regression and is **removed**, replaced by
`check_without_an_app_still_runs_the_suite`.

### The split (`crates/blastit/src/stage0.rs`, `check.rs`)

| Caller | Rule it takes |
|---|---|
| `blastit`, `blastit dev` (`app_rule`) | (a) refuse a set `BLASTIT_APP` on an all-static manifest, and (b) require a callable when the tree is not all-static |
| `blastit check` (`static_app_refusal`) | **(a) only** — an all-static manifest with a set `BLASTIT_APP` is refused by name; nothing is demanded otherwise |

```rust
pub(crate) fn static_app_refusal(config: &Config, manifest: &Manifest) -> Option<String> {
    (manifest.is_all_static() && config.app.is_some()).then(|| "BLASTIT_APP: every route is static, …")
}
pub(crate) fn app_rule(config: &Config, manifest: &Manifest) -> Result<(), String> {
    if let Some(message) = static_app_refusal(config, manifest) { return Err(message); }
    if !manifest.is_all_static() && config.app.is_none() { return Err("BLASTIT_APP: … is required"); }
    Ok(())
}
```

`check::execute` now calls only `static_app_refusal`.

### Both directions pinned (`crates/blastit/tests/critical_static_cli.rs`)

* (i) **no app is still fine** — `check_without_an_app_still_runs_the_suite`: a
  mixed (non-all-static) manifest with no `app` and no `BLASTIT_APP`, run with
  no `PATH`, reaches the suite preflight and stops there
  (`no interpreter to run pytest on`, exit `2`), and **never** names
  `BLASTIT_APP`. That proves the app rule let it through to the suite path; the
  full suite behaviour is owned by the Python contract tests above, which is why
  this case does not re-run pytest.
* (ii) **a set app is still refused** —
  `a_blastit_app_on_an_all_static_manifest_is_refused` (exit `1` for `dev`, `2`
  for `check --json`, the value absent from stderr).

### After (`make check`, exit 0)

```console
$ make check
...
PASS  docs/frameworks/static-and-spa.md  fence 8  presets   0.0s  each preset exists and fails closed without capabilities
PASS  docs/reference/cli.md  fence 1  dev-flags              0.2s  each dev flag boots and stops
PASS  docs/reference/cli.md  fence 2  check-flags            5.4s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags            0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                  0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                      0.0s  statically checked every command, flag and snippet
47 fences: 28 executed, 19 accounted, 0 failed · 55.7s
uv run --no-project tools/plan.py validate
plan OK
MAKE_EXIT=0
```

(The guard's *enforced* form, `critical_guard --base cb7a1e4 --node MB.2.6`,
reports one violation and it is **not this node's**:
` M crates/blastit-scheduler/tests/critical_static_resolve.rs [VIOLATION: not
covered by any node path]` — MB.2.4.3 is editing that protected suite right now.
`make check` reports it without enforcing, which is why the gate is green; this
node's own `A crates/blastit/tests/critical_static_cli.rs [added (always
allowed)]` is clean.)

The Python contract half (`make test-critical`, which `make check` runs):

```console
$ make test-critical
test result: ok. 12 passed; 0 failed; 0 ignored   (critical_static_cli)
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 10.166s
OK
```

And the re-run of the rest:

```console
$ cargo test -p blastit --locked
178 passed (lib) · 4 · 19 · 12 · 19 · 12 · 31 — all ok, 0 failed
$ cargo test -p blastit-py --locked
59 passed (_blastit) · 6 passed (critical_check_schema)
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ cargo fmt --all -- --check
fmt_exit=0
```
