# Result MB.21.4.2 (attempt 2)

**`STATUS: done`** — attempt 1 was **failed by review** on one MAJOR (five of the
scheduler parity suite's fourteen rows never booted a `World`, and four of them
only re-asserted `Manifest::effective_grants`, so the set stayed green if the
product handed every view every grant) and two MINORs (`dev_learn.rs` still read
the manifest with `std::fs::read_to_string` and printed `manifest.display()` on
the `--learn` write path; `scheduler/src/route.rs` promised a `[route.static]`
policy for a field `MB.21.4.1` deleted). All three are closed below, with the
fix option named and a test per fix. Every `# Acceptance` command was re-run on
the final tree: `make test-critical`, `make rs-test` and `make check` exit `0`,
the four greps print nothing, and the enforced
`critical_guard.py --base e223753 --node MB.21.4.2` is clean (2 protected
changes, both authorized). **No production code in
`crates/blastit-scheduler/src/**` changed beyond one doc comment**: the parity
suite again found no runtime gap.

## 0. What changed since attempt 1 (the review's findings)

| Finding | Fix | Test that pins it |
|---|---|---|
| **[MAJOR]** the parity suite's header, and RESULT §1, claimed "every case boots through `Options::blastfile`" while 5 of 14 rows did not | **Option B of the two the review offered**, and the one that is honest: the four grant rows are `Manifest::effective_grants`' — the scheduler carries **no grant at all** (`RouteInfo` is the name, the prefork flag, the uid and the ingress policy), so a `World` boot cannot assert one without inventing the handoff. They moved to `crates/blastit-manifest/tests/critical_blastfile_parity.rs` (the crate that owns the lowering), the scheduler file's header now says exactly that (10 rows here, 4 there, and which suite proves the runtime consumption), and RESULT §1's first sentence and those rows' cells were rewritten. The fifth row, `row_entrypoint_php_answers_every_view`, now builds a `World` and asserts the answers | the moved rows (manifest suite, 24 tests green) and the php row's three requests (scheduler suite, 10 tests green) |
| **[MINOR]** `dev_learn.rs:441,444,468` still used `std::fs::read_to_string` + `manifest.display()` | the write path goes through `crate::config::read_manifest` (i.e. `blastfile::read`, which stats before opening) and `crate::config::display_path`; `print_summary`'s path too | `dev_learn::tests::a_manifest_swapped_for_a_fifo_cannot_block_the_learn_write` — FIFO swapped in **after** the boot read, `finish` must return `1` inside 20 s; transcript in §4 |
| **[MINOR]** `route.rs:31-32` promised "no `[route.static]` policy" for a field `MB.21.4.1` deleted | clause dropped (one-line doc change) | `make rs-test` / `make check` (the doc is compiled and linted); no behaviour to test |

Nothing from attempt 1 was weakened, and no fixture was translated.

## 1. The parity suite — one named case per row, on the suite that can prove it

`crates/blastit-scheduler/tests/critical_blastfile_parity.rs` (new, protected):
**10 `#[tokio::test]`s, 10 rows, and every one of them boots a real
`common::World` through `common::Options::blastfile`** — `Manifest::from_blastfile_str`,
the real `RoutingTable`, and the file's `MountTable` handed over beside it
exactly as `stage0` hands them over. Its header maps each row to its test and
names the four rows it does **not** hold.

`crates/blastit-manifest/tests/critical_blastfile_parity.rs` (extended,
protected): **4 new `#[test]`s, the capability map's four grant rows**, moved
there because `Manifest::effective_grants` is that crate's subject, and because
the scheduler carries **no grant at all** — `RouteInfo` is the name, the prefork
flag, the uid and the ingress policy — so a `World` boot cannot assert a grant
without inventing the handoff. Its module doc now says so, and says which tests
pin which part of the runtime half: `check.rs::tests::the_route_policies_are_the_effective_grants`
(the per-route `RoutePolicySpec` list `blastit check` hands dbproxy **from a
`Blastfile`**), `stage0::tests::an_explicit_empty_grant_still_names_the_proxy_for_its_route`
(the per-route socket rule), and dbproxy's `critical_learn_and_redaction.rs`
(a `RoutePolicySpec` is the allowlist the listener enforces — it builds the spec
by hand, so it proves the last hop only). The production handoff itself,
`stage0::bind_view_sockets`, is private and privileged and **has no test**: it is
recorded as a follow-up (§ Follow-ups), not claimed.

"New" below is what this leaf adds; "already proved" names the landed suite that
pins the other half (no assertion is copied verbatim).

### 1a. The scheduler suite — 10 runtime rows, all booted

| Capability-map row | Test | New assertion | Already proved elsewhere |
|---|---|---|---|
| §9.2 static precedence, four steps | `row_static_precedence_is_the_outcome_table` | the **outcome table** (request → status) in one fixture: nested mount wins, `Missing` → view, `Hidden` (`.php` and dotfile) and `Refused` (`..`) final and never reaching a view, `fallback` on `GET`/`HEAD` only and only from the longest mount, `405` after every view, `404` otherwise | `mount_precedence.rs` (narrative precedence + body comparison), `critical_rewrite.rs` (the function) |
| §10.1 `body json` | `row_body_json_accepts_json_and_refuses_a_form_body` | JSON 200 with the view's answer; a form body → `400` before dispatch | `critical_ingress_enforcement.rs` (canonical re-encode, cap) |
| §10.1 `body form` | `row_body_form_accepts_a_form_and_refuses_json` | form 200; JSON body → `400` | same |
| §10.1 `body none` | `row_body_none_refuses_any_body` | empty body 200; one byte → `413`, no body byte at the view | same |
| §10.1 `body raw` | `row_body_raw_is_unacknowledged_and_streams` | `(Raw, false)` from the file, `RawIngress` warning, exactly one production refusal naming the view, bytes stream through | `critical_raw_ingress.rs` (production exit `1`, `dev` serves + warns) |
| §10.1 `body raw unvalidated` | `row_body_raw_unvalidated_boots_with_the_same_warning` | `(Raw, true)`, the **same** warning, empty refusal list, same bytes | same |
| §12.4 `prefork` | `row_prefork_reaches_the_route_the_scheduler_spawns` | the preforked view exists before any request; the other is spawned on first contact | `lifecycle.rs` (spawn policy) |
| §12.4 `egress any` | `row_egress_any_and_its_warning_arrive_together` | `Route.egress`, `EgressRoute`, and a served request together | the manifest parity suite's `row_a_catch_all_with_prefork_and_egress` for the warning |
| §12.2 `entrypoint php` | `row_entrypoint_php_answers_every_view` | one file-level policy, no per-view one, `can_boot_without_app()`, a mount still answered first — **and now a booted `World`**: `/` answers with view 0's scripted body, `/api/orders` with view 1's, each view saw exactly one request, and `/no-view-claims-this` is `404` (the entry is bound to the views, not to the tree) | `crates/blastit/src/php_view.rs::boot_bindings` (the binding loops the one entry per view), `critical_php_backend.rs` (a real libphp) |
| §8.4 `HEAD` implied by `GET` | `row_head_is_implied_by_get_through_the_matcher` | `RoutingTable::route("HEAD", …)` matches, `POST` does not, `methods_for` lists `GET, HEAD`, and a real `HEAD` is dispatched to the GET view | manifest parity oracle (`row_get_implies_head`) |

Deny sides are in the same tests, as acceptance #2 asks: `json`+form → `400`,
`none`+body → `413`, `raw` unacknowledged → non-empty production refusal list
plus the `dev` warning, `raw unvalidated` → boots with the same warning,
`.php`/dotfile/`..` under a mount never reach the view, a `fallback` missing
from the longest matching mount is `404` although an outer mount declares one,
`POST` to a `GET`-only view is `405`, and the php entry's unmatched path is
`404`. `§10.2`'s exact refusal wording is `MB.21.4.3`'s, so the raw case asserts
the gate's shape (one refusal, naming the view), never its nouns.

### 1b. The manifest suite — the four grant rows, and their runtime half

Moved verbatim (same names, same deny sides) from the scheduler file:

| Capability-map row | Test (manifest parity suite) | What it asserts | Runtime half, and what pins it |
|---|---|---|---|
| §11.1 a trait's grants | `row_a_trait_grants_what_the_view_receives` | `use second, first` is written order, then the inline block; `transactions` OR-ed; the inline duplicate of `SELECT 1` dropped; a trait nobody `use`s warns `UnattachedGroup` **and** grants nothing | `check::tests::the_route_policies_are_the_effective_grants` (a `Blastfile`'s per-route `RoutePolicySpec` list: the granted view carries its own queries, the ungranted one no policy) and `stage0::tests::an_explicit_empty_grant_still_names_the_proxy_for_its_route` (the per-route socket rule). The handoff itself, `stage0::bind_view_sockets`, is private and privileged: **no test** — a follow-up |
| §13.1 `postgres … unrestricted` | `row_unrestricted_on_the_default_and_a_named_store` | default and named store both `is_unrestricted()`, both `UnrestrictedRoute` warnings (route and proxy named), and `unrestricted` in a trait refused ("only allowed inline") | the same per-route selection; dbproxy `critical_learn_and_redaction.rs` proves the last hop only (it builds a `RoutePolicySpec` by hand) |
| §13.1 table and column grants | `row_table_and_column_grants` | `select products(id, name)` is a two-column `Columns::List` that does **not** contain `password`; `select products` is `Columns::All`; `insert orders`/`delete archived` as written | same (the spec carries `RoutePolicySpec.tables` unchanged) |
| §13.1 `query '…'` | `row_query_is_stored_normalised` | `$1` → `$?` in the stored statement; an inlined literal refused | same (the spec carries the normalised `queries`) |

This is exactly the division of labour the review asked for: **no test claims
more than it proves.** The scheduler suite no longer says "boots through
`Options::blastfile`" about anything that does not, and the four grant rows name
the suite that owns their lowering and, precisely, the two tests that pin the
per-route selection (plus dbproxy's last hop) — not a privileged boot nobody
tests.

## 2. The boot/`check` sentences this leaf owns (old → new subject)

Unchanged from attempt 1; re-verified green on this tree. Every string keeps its
trigger, its order and its exit code; no string owned by `MB.21.4.3` (§13.2's six
warnings, §10.2's `raw_ingress_refusals`, §11.3's nouns) was touched.

| Site | Old subject | New subject |
|---|---|---|
| `stage0::static_app_refusal` | `BLASTIT_APP: every route is static, so no route can reach an app: unset BLASTIT_APP, or declare a route without [route.static]` | `BLASTIT_APP: the file serves static mounts and no views, so no app can run: unset BLASTIT_APP, or declare a view` |
| `stage0::no_python_support` (python-less build) | `route "web": … and the route declares no [route.static] block … or add a [route.static] block` | `view "web": … and the view is reached by no \`static\` mount and no \`entrypoint php\` … or declare a \`static\` mount or an \`entrypoint php\`` |
| `check::execute` (`--learn` on a static-only file) | `nothing to learn: every route is static` | `nothing to learn: the file answers every request from static mounts` |
| `audit_v1::Report::all_static` doc | "a manifest whose every route is static" | "a manifest every request of which a `static` mount answers" |
| `check` stderr hint | `route "site": static root "public" was never requested` | `static "/site": root "public" was never requested` (also `audit_v1::unrequested_static_root_line`) |

Tests that assert the string assert the new one: `stage0::no_python_capability`
(2 tests), `stage0::app_rule_executors::the_two_halves_of_the_rule_still_hold`,
`crates/blastit/tests/critical_static_cli.rs` (3 tests),
`crates/blastit/tests/no_python_build.rs`. Comments that still said "every route
is static" in `stage0.rs` were re-spelled too, because acceptance #5 greps them.

## 3. The five MINORs — fix, test, and the failure it prevents

1. **Escaped manifest path.** One helper, `config::display_path` =
   `blastit_manifest::clipped` (escapes control bytes, cuts at 120 `char`s), used
   by `config::missing_manifest`, `ConfigError::Manifest`,
   `ManifestReadError::sentence`, and therefore by `check`, both stage-0 reads,
   stage1's re-read **and `dev --learn`'s write path** (this attempt).
   `read_manifest`'s refusals are joined onto one line (`config::errors_line`),
   because `ManifestErrors`' own `Display` is one line *per error*.
   Test: `blastfile_discovery::a_manifest_path_holding_a_newline_prints_one_escaped_line`
   (`BLASTIT_MANIFEST` = a real `./a\nb`) asserts exactly one line from `check`,
   `check --json`, `dev` and `fmt`, with `./a\nb` printed as `./a\\nb`.
   *Fails without the fix:* making `display_path` return `path.display().to_string()`
   turns `check` into two lines (`left: 2, right: 1`).
2. **A FIFO cannot block.** `config::read_manifest` is
   `blastit_manifest::blastfile::read` (stat first, refuse a non-regular file,
   `take(MAX+1)`), and every site goes through it: `config::manifest_wiring`,
   `check::read_manifest`, both `stage0` boots, `stage1`'s re-read, **and
   `dev_learn::LearnRun::finish`** (this attempt — the boot's read does not
   protect a path swapped afterwards).
   Tests: `blastfile_discovery::a_fifo_at_the_manifest_path_cannot_block_dev_or_check`
   (`check` exits `2`, `dev` exits `1`, `fmt` exits `2`, each one line naming the
   file and "not a regular file", all inside a 20 s bound) and
   `dev_learn::tests::a_manifest_swapped_for_a_fifo_cannot_block_the_learn_write`
   (a FIFO created **after** the file was read; `finish` returns `1` inside 20 s).
   *Fails without the fix:* reverting `read_manifest` blocks `check` ("did not
   exit within 20s"); reverting `dev_learn`'s read blocks `finish` (transcript in
   §4).
3. **A real `fmt` regression test.** `print::fmt` delegates to a private
   `format_with(text, layout)`; `flush` unchanged, the production caller is still
   `write`. `a_printer_regression_is_refused_rather_than_written` formats **every
   committed fixture** with `fmt` (each must come back, re-parse and be a fixed
   point) and injects a layout that writes `body xml` through `format_with`:
   `Err`, not a written string.
   *Fails without the fix:* deleting `parse(&formatted)?` lets the injected
   regression through as a string.
4. **One `pattern_is_writable`.** `audit_v1::pattern_is_writable` is
   `pub(crate)` with one doc comment (audit_v1 owns the `--json`/`--learn`
   vocabulary); `dev_learn` calls it and its byte-identical copy is deleted.
   Proof: `grep -rn 'fn pattern_is_writable' crates/blastit/src` prints one line.
5. **One static-only predicate.** `stage0::is_static_only` is
   `Manifest::is_all_static` and `stage0::runs_own_executors` is
   `Manifest::can_boot_without_app`; the model's doc comments say the
   `route.static_policy()`/`route.php()` arms are the **retired reader's
   compatibility path** that goes with `parse.rs` in `MB.21.4.5`.
   Behaviour change: an `entrypoint php` no view reaches needs no `BLASTIT_APP`.
   Test: `stage0::app_rule_executors::a_php_entrypoint_no_view_reaches_needs_no_app`
   (bare `entrypoint php`; the mount+entrypoint shape; and the deny side, the
   same view with neither).
   *Fails without the fix:* the old `runs_own_executors` refuses with
   `BLASTIT_APP: the WSGI callable (…) is required`.
   The other half: `audit_v1::unrequested_static_roots` now iterates **mounts**,
   calls a mount "requested" when an observed path the `MountTable` would answer
   for, never `stat`s, and returns `(prefix, root)`; `check` prints it as the
   hint above. Test:
   `critical_static_cli::a_static_mount_the_suite_never_requested_is_a_warning_not_a_finding`
   (allow side: a request below the prefix marks the mount requested; negative
   twin: an unreached **view** is still an `unexercised_route` finding;
   `report.ok` stays true).
   *Fails without the fix:* filtering `route.static_policy()` again yields `[]`
   where the test expects `[("/site", "public")]`.

### Evidence that each test fails without its fix (this tree)

Attempt 1's transcripts for 1, 2 (`check`/`dev`), 3, 5a and 5b were re-checked
in the attempt-1 session; the two that this attempt added were re-run here.

```console
# 2b. dev_learn reverted to std::fs::read_to_string (attempt 2)
thread 'dev_learn::tests::a_manifest_swapped_for_a_fifo_cannot_block_the_learn_write'
panicked at crates/blastit/src/dev_learn.rs:1138:14:
`learn` refused the FIFO instead of blocking on it: Timeout
test result: FAILED. 0 passed; 1 failed … finished in 20.00s
```

The experiment was reverted; `git diff` holds only the fix.

## 4. Behavioural changes beyond parsing, each with its test

| Change | Test |
|---|---|
| the manifest is read by `blastfile::read` at every entry point, **including `--learn`'s write path** (FIFO/device/16 MiB refused, never opened; a read refusal is one line) | `blastfile_discovery::a_fifo_at_the_manifest_path_cannot_block_dev_or_check`, `dev_learn::tests::a_manifest_swapped_for_a_fifo_cannot_block_the_learn_write` |
| an escaped/control-byte `BLASTIT_MANIFEST` prints one line everywhere, `learn:`'s summary included | `blastfile_discovery::a_manifest_path_holding_a_newline_prints_one_escaped_line` |
| the boot's static-only/own-executor answers are the model's `is_all_static`/`can_boot_without_app`, so `entrypoint php` with no view needs no `BLASTIT_APP` | `stage0::app_rule_executors::a_php_entrypoint_no_view_reaches_needs_no_app` |
| `unrequested_static_roots` reports mounts, "requested" = answered by the `MountTable`, no `stat`; still a stderr hint, never a finding/`ok`/exit code | `critical_static_cli::a_static_mount_the_suite_never_requested_is_a_warning_not_a_finding` |
| `check`'s `--learn` refusal, the `BLASTIT_APP`-on-static refusal and the python-less view refusal name a mount/entrypoint/view | the three suites listed in §2 |
| `HEAD`-from-`GET` **through the real matcher** (no production change — this is the proof) | `row_head_is_implied_by_get_through_the_matcher` |
| an `entrypoint php` file answers **every** view, and a path no view claims is `404` (no production change — this is the proof) | `row_entrypoint_php_answers_every_view` |

Nothing else outside parsing changed: `crates/blastit-scheduler/src/**` holds
one doc-comment edit (`route.rs`), and no `Cargo.toml`/`Cargo.lock`/`*.toml`
file appears in the diff.

## 5. Protected suites: every changed assertion and its new subject

Two protected files are modified (both authorized: `crates/blastit/tests/**` and
`crates/blastit-manifest/tests/**`), and the scheduler parity suite is **new**
(always allowed).

`crates/blastit/tests/critical_static_cli.rs`

| Test | Changed assertion | Old subject | New subject / kept contract |
|---|---|---|---|
| `an_all_static_check_refuses_learn` | the stderr string | `nothing to learn: every route is static` | `nothing to learn: the file answers every request from static mounts` (same `--learn` trigger, same exit `2`) |
| `a_blastit_app_on_an_all_static_manifest_is_refused` | the two stderr strings | `BLASTIT_APP` + `every route is static` (dev and `check`) | `BLASTIT_APP` + `the file serves static mounts and no views`; exit `1`/`2`, stdout empty, the value never printed |
| `a_static_root_the_suite_never_requested_is_a_warning_not_a_finding` → `a_static_mount_the_suite_never_requested_is_a_warning_not_a_finding` | the fixture (TOML routes → `Blastfile` mount + views), the function's argument (`RoutingTable` → `MountTable`), the expected pair | `("site", "public")` from a route's static policy | `("/site", "public")` from a mount, plus the exact stderr line, `routes.total == 1` (a mount is not a route); **added**: a request below the prefix marks the mount requested; the negative twin is now an unreached **view** that is still an `unexercised_route` finding |
| `a_missing_static_root_is_named_with_its_route_before_any_output` → `a_missing_mount_root_is_named_with_its_prefix_before_any_output` | the **name only**; every assertion unchanged (`mount "/" (index 0)`, `static root "public"`, the reason, exit `1`, empty stdout) | the name said "route" | a mount has a prefix, not a route |

`crates/blastit-manifest/tests/critical_blastfile_parity.rs`

| Change | Contract |
|---|---|
| **added**: `row_a_trait_grants_what_the_view_receives`, `row_unrestricted_on_the_default_and_a_named_store`, `row_table_and_column_grants`, `row_query_is_stored_normalised` | the four grant rows moved out of the scheduler suite, assertions unchanged (order, dedup, OR-ed `transactions`, both warnings, the trait refusal, the literal refusal, `Columns::List` vs `All`, `$1` → `$?`) — §1b |
| **added**: helpers `lower_source`, `warnings_of`, `has_warning`; imports `ManifestWarning`, `UidLayout` | support only; no existing test touched |
| module doc | now says the file also carries the capability map's four grant rows and names the runtime seam; no assertion changed |

No assertion was deleted or weakened: exit codes, empty stdout, "never a
finding", "`ok` stays true" and every refused/accepted pair are kept.

Non-protected test files extended (in `paths`):
`crates/blastit/tests/blastfile_discovery.rs` (2 new cases + a bounded-run
helper), `crates/blastit/tests/no_python_build.rs` (the sentence's nouns), and
the in-crate tests of `stage0.rs`/`print.rs`/`dev_learn.rs` (the new FIFO case).

## 6. Acceptance evidence

All commands in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` on
the final tree, with `timeout`, in the foreground, all four together after the
last edit (the doc comments naming the grant rows' runtime half and this file).
`make check` was also run twice earlier in the session; every run exits `0` with
the same 166 `test result: ok` lines.

The node's numbered items map to the evidence like this:

| Node item | Where |
|---|---|
| 1 — parity suite boots through `Options::blastfile`, one case per row, header table; RESULT says per row whether it is new or already proved | §1a (10 runtime rows, all booted) and §1b (the 4 rows that live on the suite that owns their lowering, with the runtime half named) |
| 2 — every precedence/refusal claim has a deny side | §1a's last paragraph (the same 10 tests) and §1b's rows |
| 3 — static-only file boots and `check`s without `BLASTIT_APP`; a missing/unreadable root names the prefix; an unreached mount is a one-line stderr hint and only a hint | `critical_static_cli::{an_all_static_site_boots_under_dev_with_no_app, an_all_static_check_audits_the_root_and_runs_no_pytest, an_all_static_check_exits_two_on_a_missing_root, a_missing_mount_root_is_named_with_its_prefix_before_any_output, a_root_the_caller_cannot_list_is_refused, a_static_mount_the_suite_never_requested_is_a_warning_not_a_finding}` — 12/12 green below |
| 4 — each MINOR has a test that fails without its fix; one fixture set pins the predicate arms | §3, with the transcripts |
| 5 — the four greps | the empty commands below |
| 6 — `make test-critical`, `make rs-test`, `make check` exit `0`, guard clean | the command blocks below |
| 7 — no fixture translated | `git diff --stat HEAD -- '*.toml'` prints nothing (below); no file renamed |
| 8 — every behavioural change named with its test | §4 |

### 1. `make test-critical` — exit 0, zero skips, the new suite picked up

```console
$ timeout 1800 make test-critical ; echo EXIT=$?
cargo test -p blastit-manifest --test critical_blastfile_parity
running 24 tests            # 4 of them this leaf's grant rows
test row_a_trait_grants_what_the_view_receives ... ok
test row_unrestricted_on_the_default_and_a_named_store ... ok
test row_table_and_column_grants ... ok
test row_query_is_stored_normalised ... ok
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
cargo test -p blastit-scheduler --test critical_blastfile_parity
running 10 tests
test row_entrypoint_php_answers_every_view ... ok
test row_static_precedence_is_the_outcome_table ... ok
…
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
…
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 3.722s
OK
EXIT=0
```

(41 `test result: ok` lines, no `ignored; [1-9]`, no failure.)

### 2. `make rs-test` — exit 0

```console
$ timeout 3600 make rs-test ; echo EXIT=$?
cargo test --workspace --all-targets --locked --features python   # 122 `test result: ok`, 0 failed
cargo test --workspace --doc --locked --features python            # all doctests ran
EXIT=0
```

### 3. `critical_guard.py --base e223753 --node MB.21.4.2` — clean

```console
$ timeout 600 uv run --no-project tools/critical_guard.py --base e223753 --node MB.21.4.2
critical-guard: e223753..working tree, 19 changed file(s)
  M  crates/blastit-manifest/tests/critical_blastfile_parity.rs  [authorized by crates/blastit-manifest/tests/**]
  M  crates/blastit/tests/critical_static_cli.rs  [authorized by crates/blastit/tests/**]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.4.2
```

(Inside `make check` the target is report-only and passes no `--node`, so it
prints the same two protected changes as "VIOLATION: not covered by any node
path"; with `--node` — the enforced run above — they are authorized.)

### 4–7. The four greps and the seventh command — all empty

```console
$ grep -rnE 'Route::with_static|fn with_static\(|fn static_policy\(|fn with_php\(|SpaLost|static_forward' \
    crates/ --include='*.rs' | grep -vE '^crates/blastit-manifest/src/(model|parse)\.rs:'
(no output)   # only model.rs/parse.rs lines existed, and they are filtered: MB.21.4.5's

$ grep -rn 'every route is static' crates/blastit/src crates/blastit/tests
(no output)

$ grep -rn 'route "site"' crates/blastit/tests/
(no output)

$ git diff --stat HEAD -- '*.toml'
(no output)   # this leaf translated no fixture
```

One `pattern_is_writable`:

```console
$ grep -rn 'fn pattern_is_writable' crates/blastit/src
crates/blastit/src/audit_v1.rs:1343:pub(crate) fn pattern_is_writable(path: &str) -> bool {
```

### 8. `make check` — exit 0

```console
$ timeout 5400 make check ; echo EXIT=$?
guard: OK (17 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 19 changed file(s)     # report-only here
cargo fmt --all -- --check                                 # clean
cargo clippy --workspace --all-targets … -- -D warnings    # Finished, 0 warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps …  # Finished, 0 warnings
cargo test --workspace --all-targets --locked --features python  # 166 `test result: ok`, 0 failed
…test-critical (crates/*/tests/critical_*.rs + python critical)…
49 fences: 28 executed, 21 accounted, 0 failed · 17.7s
…rs-no-python, seam-check, platform-matrix, workflow-check, packaging-gates, doc-tone,
  manifest-purity (17 sources, no std::env)…
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The only `warning:` line in the log is cargo's pre-existing "doc tests are not
supported for crate type(s) `cdylib` in package `blastit-py`". `make check`
includes `make rs-test` and `make test-critical` verbatim (line 774 of the
Makefile), so its legs are the last-tree witness for the standalone runs above.

## 7. Files outside `paths`, and the plan tree

`docs/knowledge/LESSONS.md` (one line, extended this attempt with the harness's
per-view request ids). Everything else modified in `plan/` or `docs/research/`
was already dirty when this attempt started (the dispatcher's `status`/
`attempts`, `plan/BUDGET.md`, `plan/JOURNAL.md`,
`docs/research/2026-11-16-zed-blastfile-lsp.md`, `docs/research/INDEX.md`,
`plan/nodes/MB/MB.22/`) and was not touched. Attempt 1's `NODE.md` re-word is
still there because `docs-unit`'s retired-vocabulary guard fails on the original
clause; no frontmatter was hand-edited.

## Follow-ups (out of this leaf's `paths`)

- **`EntrypointNotReached` still does not exist.** §9.5/§12.2 and acceptance #4
  ask for a warning, but the variant and its sentence live in
  `crates/blastit-manifest/src/validate.rs` — outside this leaf's `paths` and
  assigned by §13.2 to `MB.21.3.0` — and `critical_blastfile_entrypoint.rs:100`
  records the same gap. This leaf pins the half it owns: `entrypoint php` with no
  view needs no `BLASTIT_APP`, is not all-static, and every view answers.
  `MB.21.4.3`/`MB.21.4.5` should add the variant + emission, and the scheduler
  parity suite's php row should then assert it.
- **The grant rows have no runtime assertion in a suite this leaf may extend.**
  `stage0::bind_view_sockets` (the production handoff that turns a view's
  `effective_grants` into a `ListenerSpec`) is private and needs stage 0's
  privileges, so **it has no test at all**, and `blastit-dbproxy/tests/**` is not
  in `paths`. What exists is the per-route selection on the `check` path
  (`check::tests::the_route_policies_are_the_effective_grants`, asserted on a
  `Blastfile`), the socket rule (`stage0::tests::an_explicit_empty_grant_still_names_the_proxy_for_its_route`)
  and dbproxy's enforcement of a hand-built `RoutePolicySpec`. A later leaf
  should either give `bind_view_sockets`' selection a pure, testable function of
  `(manifest, index)` and assert it, or add a dbproxy test that boots a
  `Blastfile`'s grant map end to end; then the four rows' runtime half becomes an
  assertion rather than a citation.
- **`crates/blastit/src/cli.rs::one_line`** is a second name for
  `blastit_manifest::clipped`; `cli.rs` is outside `paths`. Collapse it onto
  `config::display_path` in the leaf that owns `cli.rs`.
- **`crates/blastit-manifest/src/lib.rs:26`** still says "is needed once every
  route is static" in its module docs (outside `paths`); the `MB.21.4.5` sweep
  should re-spell it.
- **§10.2's `raw_ingress_refusals`** still uses the retired spelling
  (`allow_raw = true`, `[route.ingress]`), exactly as `MB.21.4.3` owns; the
  parity suite asserts only that there is one refusal naming the view.
- **`docs/design/static-and-spa.md:809,819`** quote the old hint/sentence; they
  are dated records, so the choice of whether to annotate them belongs to
  `MB.21.5.x`.
