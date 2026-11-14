# RESULT MB.3.5.4 — `[route.php]` grammar (`PhpPolicy`, `Executor::Php`)

Base `5dcf16d`, branch `release/0.2.1`, attempt 1. Manifest crate only.

## What landed

- `PhpPolicy { entry: String, workers: Option<u32> }` (public fields, `Clone/Debug/PartialEq/Eq`);
  `workers` is optional and a **downward cap only**, no `listen`, no pool sizing.
- `Executor::Php(PhpPolicy)` — a typed plan, not a backend string; `kind() == "php"`;
  `ALL_KINDS == ["wsgi", "php"]`. `Executor::View` is unchanged.
- `Route::php() -> Option<&PhpPolicy>` (+ `pub(crate) with_php`), parsed from `[route.php]`
  (a reserved route key like `static`, so `[proxy.php]` can never be read as the block and
  vice versa). `[route.fastcgi]` stays an unknown key naming itself.
- Rules inherited verbatim from the retired key (`validate::check_php`): relative `.php`
  entry, no `..`, no `\`, no control byte, optional `workers` in `1..=64`, `spa = true`
  conflict, `CONNECT`/`TRACE` refusal, plus **`entry` under `static.root` when both blocks
  are set** (whole-segment compare, so `publicity/` is not beneath `public/`; `root = "."`
  covers the manifest directory).
- New variants `PhpEntryInvalid { location, entry, rule: String }`,
  `PhpWorkersInvalid { location, workers, why }`, `PhpDeadDeclaration { location, key, why }`
  with the design's message shape (`route "app" (index 0): php entry …: <rule>`).
- Boot predicates: `can_boot_without_app()` accepts a `[route.php]` route (its own
  executor); `is_all_static()` is `false` for a PHP route **and** for a route declaring both
  blocks (the naive `all(static_policy().is_some())` said `true` and would have let `check`
  report "nothing to learn" for a PHP route). An empty or WSGI-only manifest still refuses.
- `FastCgiListenDerived`/`FastCgiPoolSocketInvalid` do not exist anywhere (`grep FastCgi
  crates/` is empty); no dead variant was reintroduced.

Tests: new protected `tests/critical_php_manifest.rs` (10 tests, allowed **and** denied per
rule, plus the exact three messages and the fastcgi-unknown-key twin), `tests/parse.rs`'s
retired-key test now carries the `[route.php]` accepted twin,
`model.rs`'s kind/predicate units updated.

Scope: `docs/**` untouched, `Cargo.toml`/`Cargo.lock` untouched, `docs_fences.rs` untouched
(the `STALE` row stays for `MB.3.20`), no plan frontmatter, no commit.

## Acceptance evidence

```
$ cargo test -p blastit-manifest --locked
   Running tests/validate.rs ...
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
   Doc-tests blastit_manifest
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

all doctests ran in 0.25s; merged doctests compilation took 0.24s
```
(Every suite green, including `critical_php_manifest` 10/10, `critical_static_manifest` 15/15,
`critical_manifest_edit` 23/23, `parse` 34/34, `docs_fences` 6/6.)

```
$ cargo test -p blastit-manifest --test docs_fences --locked
test the_stale_accounting_names_only_manifests_that_still_fail ... ok
test every_documented_manifest_parses_and_validates ... ok
test a_drifted_static_block_does_not_satisfy_the_documented_shape ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

```
$ cargo test -p blastit --locked
exit=0 ; "test result: ok" lines: 11 ; "test result: FAILED" lines: 0
test result: ok. 183 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.29s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```
$ make check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
...
interpreter-free build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
exit=0 ; "test result: ok" lines: 113 ; "test result: FAILED" lines: 0
```

Surface:

```
$ grep -rn 'route.php\|PhpPolicy\|Executor::Php' crates/blastit-manifest/src/ | head
crates/blastit-manifest/src/error.rs:488:    /// A `[route.php]` `entry` is not a relative `.php` path beneath the
crates/blastit-manifest/src/error.rs:499:        /// Where the `[route.php]` block is written.
crates/blastit-manifest/src/error.rs:506:    /// An explicit `[route.php]` `workers` cap is outside `1..=64`
crates/blastit-manifest/src/error.rs:514:        /// Where the `[route.php]` block is written.
crates/blastit-manifest/src/error.rs:521:    /// A `[route.php]` route carries a declaration that never reaches the front
crates/blastit-manifest/src/error.rs:529:        /// Where the `[route.php]` block is written.
crates/blastit-manifest/src/lib.rs:18://!   and [`Route::php`] the `[route.php]` front controller an embedded `libphp`
crates/blastit-manifest/src/lib.rs:78://! the embedded worker ([`PhpPolicy`]), not the retired pool.
crates/blastit-manifest/src/lib.rs:94:    Executor, Grant, Group, Manifest, PgGrant, PhpPolicy, Protocol, Proxy, ProxyName, Route,
crates/blastit-manifest/src/model.rs:321:/// A `[route.php]` block: the front controller a route's embedded PHP workers
crates/blastit-manifest/src/model.rs:330:pub struct PhpPolicy {
crates/blastit-manifest/src/model.rs:359:    Php(PhpPolicy),
crates/blastit-manifest/src/model.rs:368:    /// for [`Executor::View`], `"php"` for [`Executor::Php`].
```

```
$ git diff 5dcf16d -- crates/blastit-manifest/tests/docs_fences.rs | wc -l
0
```

```
$ grep -rn "FastCgiPoolSocketInvalid\|FastCgi" crates/   # empty
$ git diff 5dcf16d --stat -- Cargo.toml Cargo.lock crates/blastit-manifest/Cargo.toml | wc -l
0
```

## Follow-ups (out of this node's `paths`)

1. `crates/blastit/src/stage0.rs::app_rule` binds the app rule to `is_all_static()`, not
   `can_boot_without_app()`, and `no_python_support` uses `is_all_static()` too: a
   **PHP-only** manifest would therefore be refused with
   "BLASTIT_APP: the WSGI callable (`module:attribute`) is required" (and, in a build
   without `python`, with the `uv add blastit` sentence) even though the route is its own
   executor. The manifest-level predicate is correct now; the stage-0 wiring is
   `MB.3.5.2`'s ("boot predicates") — its acceptance must assert the PHP-only case boots
   without `BLASTIT_APP` and never prints `uv add`.
2. `is_all_static()` is read by `check.rs` ("nothing to learn: every route is static") and
   `stage0::static_app_refusal`; both now correctly stay out of the way for a PHP route —
   re-check them in `MB.3.5.2`/`MB.3.20` contexts.
3. `edit.rs::expected_routes` rebuilds a route with `Route::new(..).with_ingress(..)`, so it
   drops `static_policy` (pre-existing) and will also drop a `php` block once `--learn`
   rewrites such a manifest. `edit.rs` is out of this node's paths; `MB.3.10`/`MB.3.20`
   should either fix the postcondition or prove `--learn` is never reached for those routes.

## Attempt 2 — the boot decisions now key on the executor, not `is_all_static`

Reviewer gap: `stage0::app_rule` and `stage0::no_python_support` read "not
all-static" as "needs a WSGI app", so a **PHP-only** manifest was refused with
`BLASTIT_APP: the WSGI callable (…) is required` — and, in the Python-free
build, with the `uv add blastit` sentence. Both now ask
`Manifest::can_boot_without_app()` ("every route runs its own executor"):

- `app_rule`: `static_app_refusal` first (unchanged), then a refusal only when
  `!can_boot_without_app() && config.app.is_none()`. PHP-only and static+PHP
  boot; empty / WSGI-only / php-plus-WSGI still refuse with the exact sentence,
  which never mentions `uv add`.
- `no_python_support`: early `None` for `can_boot_without_app()` (so PHP-only is
  not refused by the capability gate), and the "route that needs a runtime this
  build does not carry" search now skips `[route.php]` routes as well as
  `[route.static]` ones — `uv add` is kept for a genuinely WSGI-shaped route.

Tests added (grammar and predicates untouched otherwise):

| Case | Where | Asserts |
|---|---|---|
| (i) PHP-only boots | `stage0::app_rule_executors::a_php_manifest_boots_without_an_app` | `app_rule(...) == Ok(())` for php-only and static+php |
| (ii) no app + no executor refuses | `…::a_manifest_with_no_executor_still_requires_the_callable` | exact sentence, and `!contains("uv add")`; includes php + WSGI view (one own-executor route cannot excuse the manifest) |
| (iii) Python-free PHP-only is not refused | `no_python_capability::a_php_only_manifest_is_not_refused_by_the_capability_gate` (unit, both builds) and `no_python_build::a_php_only_manifest_is_not_refused_by_the_capability_gate` (real binary, `--no-default-features --features php`) | no `CAPABILITY_GATE` / `INSTALL` / `APP_RULE` sentence; the real-binary run reaches the banner (`1 route`) and only the held-port bind fails |

The real binary's stdout is not empty on that path (the banner and events are
printed before the bind), which is why that case asserts the banner and the
absent sentences rather than §1.1's empty-stdout-for-a-refusal.

Manual check that the default build really boots it:

```
$ BLASTIT_MEM_HIGH_WATERMARK=100 timeout 20 target/debug/blastit dev --port 43991   # php-only manifest
  blastit dev · single-uid simulation · 127.0.0.1:43991
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
… INFO  ready       serving  routes=1  workers=16
exit=124   # `timeout` killed a serving server; no BLASTIT_APP refusal
```

### Attempt-2 acceptance

```
$ make check
interpreter-free build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
exit=0 ; "test result: ok" lines: 113 ; "test result: FAILED" lines: 0

$ cargo test -p blastit --locked
exit=0 ; "test result: ok" lines: 11 ; "test result: FAILED" lines: 0

$ cargo test -p blastit-manifest --locked
exit=0 ; "test result: ok" lines: 14 ; "test result: FAILED" lines: 0

$ make rs-no-python
cargo test  -p blastit --no-default-features --features php --lib
test result: ok. 163 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
cargo test  -p blastit --no-default-features --features php --locked --test no_python_build
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
cargo test -p blastit-view --no-default-features --locked --lib
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
exit=0

$ git diff 5dcf16d -- crates/blastit-manifest/tests/docs_fences.rs | wc -l
0
```

Unchanged from attempt 1: `docs/**` untouched, no `Cargo.toml`/`Cargo.lock`
change, no plan frontmatter, no commit. Follow-ups (1) and (iii) are now closed
in-tree; follow-up (2) (`edit.rs::expected_routes` dropping `static`/`php` on a
`--learn` rewrite) stays `MB.3.6.4`'s.
