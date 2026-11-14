# RESULT MB.2.5.1 — rewrite precedence and the static method/body gate

STATUS: done

Base `7a79a95` (`release/0.2.0`). Four files, exactly the node's `paths`:

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/rewrite.rs` | **new**: `Rewrite`, `decide`, `STATIC_ALLOW`, `declares_body` (166 production lines) |
| `crates/blastit-scheduler/src/route.rs` | `RouteInfo::{static_policy, static_forward}` + `with_static(policy, has_app)`; `routes_from_manifest` passes it; two unit tests |
| `crates/blastit-scheduler/src/lib.rs` | `pub mod rewrite;` |
| `crates/blastit-scheduler/tests/critical_rewrite.rs` | **new** protected suite, pure half (13 tests) |

`static_asset.rs`, `static_response.rs`, `scheduler.rs`, `config.rs` and
`tests/common/static_fixture.rs` are untouched (`git status` above shows only the
four files). Every case is driven through the production composition
`head_segments` → `resolve` → `decide` over the §5.2 fixture tree, never a
hand-made `StaticFile`.

## Total order as implemented (§4.3 steps 5–11, §4.4)

`decide(outcome, method, has_body, has_app)` is total:

| `StaticOutcome` | `GET`/`HEAD`, no body | `GET`/`HEAD`, declared body | any other method |
|---|---|---|---|
| `File(_)` | `Serve(file)` | `Reject 413` | `Reject 405`, `allow = Some(b"GET, HEAD")` |
| `Missing` + `has_app` | `Forward` | `Forward` (ingress decides) | `Forward` |
| `Missing` + no `has_app` | `Reject 404` | `Reject 404` | `Reject 404` |
| `SpaLost` / `Hidden` | `Reject 404` | `Reject 404` | `Reject 404` |
| `Refused(_)` (all 7 variants) | `Reject 400` | `Reject 400` | `Reject 400` |

The method gate runs **before** the body gate (`POST` + body on a static target is
`405`, never `413`). `Rewrite::Reject` carries `allow` only for the `405`.

## Precedence table (pair → winner → test that asserts both ways)

| Pair | Winner | Both-way assertions |
|---|---|---|
| app route vs. static route (§4.3 steps 1–2, "two routes, one path") | first route in manifest order; `[static]` adds no rule | `an_app_route_matches_before_a_later_static_route`: app-catch-all first ⇒ `route == 0` with `static_policy == None` / `static_forward == false`; swapped ⇒ `route == 0` is the static one |
| static file vs. front controller | the file | `an_exact_file_is_served_for_every_alias_and_never_forwarded` (`has_app` false **and** true ⇒ `Serve(Exact)`); `a_miss_forwards_only_with_an_app…` (miss + app ⇒ `Forward`) |
| static file vs. SPA fallback | the file | `a_miss_on_a_spa_route_answers_index_and_never_the_near_miss_file` (`Serve(SpaFallback)` for misses) vs. exact/dir answers in the two tests above; near-miss NFD ⇒ index, never `café.js` |
| directory index | `Serve(DirectoryIndex)`, `/docs` = `/docs/` | `a_directory_answers_its_index_and_both_spellings_are_one_file` (same `url_path`, same bytes) |
| SPA fallback vs. front controller | the fallback | `a_miss_on_a_spa_route…` with `has_app = true` ⇒ `Serve(SpaFallback)`, never `Forward`; same miss with `spa` unset ⇒ `Forward` |
| `spa = false` vs. fallback | `404` | `a_route_without_spa_answers_404_instead_of_falling_back` (misses `404`, `/` and `/docs` still served — not a blanket 404) |
| lost fallback (`SpaLost`) vs. app/fallback | `404` | `a_lost_spa_fallback_is_a_404_and_never_the_app` (index removed on disk; `has_app = true`, every method/body) |
| refusal vs. app/fallback/index | the refusal | `a_refusal_is_final_even_with_a_spa_fallback_and_an_app_armed` (rows 1–13 `400` with the exact `StaticRefusal`; rows 15–17, 20–26, 39 `404`; `has_app = true` and `spa = true` armed throughout) |
| nothing beats an app route | `Forward` untouched | `a_miss_forwards_only_with_an_app…` (all 9 method shapes + declared body ⇒ `Forward`) |
| nothing beats a refusal | `Reject` | `the_status_mapping_is_total…` (9 methods × 2 bodies × 2 `has_app` × all outcomes; `Serve` only for `GET`/`HEAD` without a body) |

Status mapping asserted for every refusal: `400` all seven `StaticRefusal`
variants, `404` `Hidden`/`SpaLost`/`Missing`-without-app, `405` with exactly
`Allow: GET, HEAD` (and no `Allow` on `400`/`404`/`413`), `413` for a `GET`/`HEAD`
that declares a body.

Method/body gate: `405` for `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `get`,
`""` on a file, a directory index, `/`, the SPA fallback (`POST /dashboard` on a
`spa = true` route) and the `.well-known` token; `413` for `GET`/`HEAD` with a
declared body; `HEAD` selects the same file as `GET`.
`declares_body` is table-driven (`Content-Length: 0`/`00` ⇒ false, `1`/`1024` ⇒
true, any `Transfer-Encoding` ⇒ true, `X-Content-Length` and `Content-Type`
ignored, unparsable length ⇒ true, fail closed), with header names compared
ASCII case-insensitively.

## Protected-test teeth (mutants applied by hand, reverted each time)

| Mutant | Result |
|---|---|
| `decide` returns `Forward` first | FAILED: 3 passed, 10 failed |
| method gate never fires (file always `Serve`) | FAILED: 10 passed, 3 failed |
| body gate never fires | FAILED: 11 passed, 2 failed |
| a miss always forwards | FAILED: 9 passed, 4 failed |
| `Hidden` answered `400` | FAILED: 11 passed, 2 failed |
| `SpaLost` answered `400` | FAILED: 11 passed, 2 failed |
| `declares_body` ignores `Content-Length` | FAILED: 12 passed, 1 failed |
| header name compared case-sensitively | FAILED: 12 passed, 1 failed |
| `static_forward` ignores `spa` | FAILED: 44 passed, 1 failed (lib) |

`rewrite.rs` matches the design exactly (`Rewrite`, `decide`); no status outside
§4.4's four is produced.

## Acceptance evidence

### `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked`

```console
running 13 tests
test a_static_policy_can_forward_only_without_spa_and_with_an_app ... ok
test a_miss_on_a_spa_route_answers_index_and_never_the_near_miss_file ... ok
test a_directory_answers_its_index_and_both_spellings_are_one_file ... ok
test a_lost_spa_fallback_is_a_404_and_never_the_app ... ok
test a_miss_forwards_only_with_an_app_and_keeps_every_method_and_body ... ok
test declares_body_reads_the_head_and_never_a_body ... ok
test a_route_without_spa_answers_404_instead_of_falling_back ... ok
test a_get_or_head_that_declares_a_body_is_413_and_the_method_gate_runs_first ... ok
test an_exact_file_is_served_for_every_alias_and_never_forwarded ... ok
test a_refusal_is_final_even_with_a_spa_fallback_and_an_app_armed ... ok
test the_status_mapping_is_total_over_every_outcome_and_every_request ... ok
test the_method_gate_is_405_with_exactly_allow_get_head ... ok
test an_app_route_matches_before_a_later_static_route ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked`

```console
test route::tests::static_forward_is_a_policy_without_spa_and_with_an_app ... ok
test route::tests::a_route_without_ingress_takes_no_body_and_with_ingress_keeps_its_codec ... ok
test route::tests::routes_follow_manifest_order_and_the_uid_layout ... ok
test route::tests::routes_from_manifest_carries_the_policy_and_the_app_rule ... ok
test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### `make rs-lint`

```console
cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Checking blastit v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit)
    Checking blastit-py v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.42s
```

### `uv run --no-project tools/critical_guard.py --base 7a79a95 --node MB.2.5.1`

```console
critical-guard: 7a79a95..working tree, 6 changed file(s)
node MB.2.5.1: paths ['crates/blastit-scheduler/src/rewrite.rs', 'crates/blastit-scheduler/src/route.rs', 'crates/blastit-scheduler/src/lib.rs', 'crates/blastit-scheduler/tests/critical_rewrite.rs']
  no change to a protected critical test
```

### `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked`

```console
test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s   (lib)
test result: ok. 11 passed; 0 failed; ... 0.03s   (admission)
test result: ok. 9 passed; 0 failed; ... 0.00s    (critical_ingress_enforcement)
test result: ok. 13 passed; 0 failed; ... 0.01s   (critical_rewrite)
test result: ok. 14 passed; 0 failed; ... 0.01s   (critical_static_resolve)
test result: ok. 9 passed; 0 failed; ... 0.00s    (ctl_channel)
test result: ok. 17 passed; 0 failed; ... 0.07s   (fairness)
test result: ok. 6 passed; 0 failed; ... 0.06s    (flow)
test result: ok. 2 passed; 0 failed; ... 0.09s    (head_allocs)
test result: ok. 13 passed; 0 failed; ... 0.04s   (isolation)
test result: ok. 18 passed; 0 failed; ... 0.04s   (lifecycle)
test result: ok. 14 passed; 0 failed; ... 0.03s   (memory)
test result: ok. 14 passed; 0 failed; ... 0.01s   (routing)
   Doc-tests blastit_scheduler
test result: ok. 1 passed; 0 failed; ... 0.00s
all doctests ran in 0.19s; merged doctests compilation took 0.19s
```

### `cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings`

```console
    Checking blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.83s
```

### `RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler --no-deps --locked`

```console
 Documenting blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.56s
   Generated /home/remy/offworld-nexus/blastit/target/doc/blastit_scheduler/index.html
```

### `cargo fmt --all -- --check`

```console
exit=0
```

## Deviations (named)

1. **Line budget.** NODE allows ≤ 4 files / ≤ 600 changed lines. Files: 4. Lines:
   166 (`rewrite.rs`) + 88/3 (`route.rs`) + 1 (`lib.rs`) + 672
   (`critical_rewrite.rs`) = **927**. Production is 166 + 88 + 1 = 255 and
   `rewrite.rs` is well under its 250-line cap; the overrun is the protected
   suite, which has to carry the ten behaviours §7.2 assigns to
   `critical_rewrite.rs` plus the accepted twins that make a constant `decide`
   fail. Nothing was truncated to fit.
2. **One function beyond the design's named surface.** §4.4 defines `has_body`
   ("`Content-Length > 0` or any `Transfer-Encoding`") but names no function.
   `rewrite::declares_body` implements that definition next to `decide` so the
   gate's input is written and tested in one place instead of being re-derived
   in `MB.2.5.2` (which is not a testable-from-here path). It cannot produce a
   status and adds no public type.
3. **`SpaLost`/`Hidden`/`Refused` on a non-`GET` method.** §4.4's table has rows
   only for "static target selected" and "no target"; steps 8/10/11 make these
   three *final*, so `decide` answers `404`/`404`/`400` for every method rather
   than `405`. No new status invented; the suite pins it in
   `the_status_mapping_is_total…` and `a_lost_spa_fallback_is_a_404…`.
4. **Unparsable `Content-Length`.** `declares_body` treats a value it cannot
   read as a declared body (`413` on a static target) — fail closed for a head
   the front end should have refused first. Named in the function's rustdoc.
5. **`make critical-guard BASE=develop NODE=MB.2.5.1` fails on this branch** with
   4 violations in files this node never touched
   (`critical_manifest_edit.rs`, `blastit/tests/critical_check_engine.rs`,
   `critical_config_and_presets.rs`, `python/tests/critical/test_check_contract.py`).
   They are earlier release-branch changes against `develop` (252 changed files
   overall), not this leaf: the node's own base `7a79a95` gives
   `no change to a protected critical test` (evidence above), and `git status`
   shows only this node's four files. Pre-existing for every node on
   `release/0.2.0`; left alone as out of lane.
6. **One incidental fmt touch, reverted.** An early `cargo fmt --all` rewrote
   `tests/common/static_fixture.rs` (MB.2.4.1's frozen fixture, out of lane). It
   was restored byte-for-byte from `HEAD`; the final `cargo fmt --all` is a
   no-op on it and `cargo fmt --all -- --check` exits `0`.
7. `docs/knowledge/LESSONS.md` was **not** edited: it is not in this node's
   `paths`. No reproducible non-obvious gotcha survived the investigation above
   (the fixture rewrite is not reproducible once reverted), so nothing is lost.

## Follow-ups

- `MB.2.5.2` owns `critical_rewrite.rs` next: it appends the flow half
  (`dispatch_static`, slots, `static_forward` spawn rule, `413`/`405` on the head,
  `slot_releasable`). It must call `rewrite::decide` and `rewrite::declares_body`
  rather than re-deriving either, and pass
  `RouteInfo::static_forward`/`static_policy` from `routes_from_manifest`.
- `MB.2.6` still has to chain `.with_static(...)` on the three direct `RouteInfo`
  builders in `stage1`; `routes_from_manifest` now already passes it.
- The `plan/` working-tree modifications (`BUDGET.md`, `JOURNAL.md`, `LOG.md`,
  `NODE.md`) were made by the plan tooling before this attempt; not touched here.
