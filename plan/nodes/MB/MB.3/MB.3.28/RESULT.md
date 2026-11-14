# RESULT MB.3.28 — interim `[route.php]` refusal (boot **and** check)

## Status

`done`. `[route.php]` fails closed in stage 0 on both boot paths and in
`blastit check`, before a listener is bound or anything is served; it can no
longer reach `Ctl::Spawn` (which would `fail()` the container in the standalone
build or fork a WSGI view in the wheel build). `MB.3.13.3` deletes the guard
when `SpawnSpec::Php` is wired.

## The sentence

Exactly one witness, no alternatives named (owner invariant: "NO FALLBACK — one
and only one way to run PHP"):

```text
route "app": the embedded PHP backend is not available in this build ([route.php] entry "public/index.php"); the per-route worker wiring lands in MB.3.13.3
```

The unit test and every protected refusal case assert the sentence contains
none of `php-fpm`, `fastcgi`, `[route.fastcgi]`, `WSGI`, `meanwhile`, `libphp`
(case-insensitive), and that it still names the wiring (`MB.3.13.3`) — so
dropping the wiring half cannot stay green.

## Changes (paths only)

- `crates/blastit/src/stage0.rs`
  - Added `pub(crate) fn php_backend_unavailable(manifest: &Manifest) ->
    Option<String>`, sibling of `no_python_support`: the first route with a
    `[route.php]` block yields the sentence above. The doc comment records that
    `ADR-0010` §1 gives PHP one backend, so the sentence names no alternative,
    and that `MB.3.13.3` deletes the function and all three call sites.
  - Called it in `run` (production boot) and `run_dev` (`blastit dev`), after
    the python-capability gate and **ahead of** `app_rule`. Both are exit `1`
    with empty stdout in `dev`.
  - Added `php_backend_gate` unit tests (first PHP route named, static/WSGI
    twins return `None`, sentence has the route/entry/wiring, **and none of**
    `php-fpm`/`fastcgi`/`route.fastcgi`/`wsgi`/`meanwhile`/`libphp`). Updated
    the now-stale comments in `app_rule`/`no_python_support` and the two
    existing PHP unit tests.
- `crates/blastit/src/check.rs`
  - Calls `php_backend_unavailable(&manifest)` beside `static_app_refusal`,
    returning `Err(message)` so `check` exits `2` (its configuration-error code)
    with nothing on stdout — before pytest, dbproxy or any serve path.
- `crates/blastit/tests/critical_php_backend.rs` (new, protected)
  - `a_php_route_is_refused_at_boot_and_never_reaches_a_wsgi_view`: PHP-only
    manifest under `blastit dev`, held port + `env_clear` +
    `BLASTIT_MEM_HIGH_WATERMARK=100`; exit `1`, **stdout empty**, stderr has
    `route "app"`, `[route.php] entry "public/index.php"`, the sentence and
    `MB.3.13.3`; asserts no fallback recipe (six banned tokens) and **no**
    `single-uid simulation` banner or `view …` line.
  - `check_refuses_a_php_route_before_anything_is_served`: `blastit check`
    exits `2`, stdout empty, same sentence + `MB.3.13.3`, same absence
    assertions.
  - `a_static_and_php_manifest_is_refused_by_the_same_sentence`: the documented
    page shape is refused by the same sentence + `MB.3.13.3`, same absence
    assertions.
  - `a_static_only_manifest_is_not_refused_by_this_gate`: allowed twin; the
    boot passes the gate (banner, `1 route`) and only the held port refuses it,
    and the sentence is absent.
- `crates/blastit/tests/no_python_build.rs`
  - `a_php_only_manifest_is_refused_by_the_interim_php_gate`: exit `1`, empty
    stdout, the sentence, and still **not** the capability gate's "install the
    Python package" or `app_rule` sentences. Unchanged by the wording edit.

## Acceptance

### `cargo test -p blastit --test critical_php_backend --locked`

```console
$ cargo test -p blastit --test critical_php_backend --locked
CRIT_EXIT:0
running 4 tests
test check_refuses_a_php_route_before_anything_is_served ... ok
test a_php_route_is_refused_at_boot_and_never_reaches_a_wsgi_view ... ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test a_static_and_php_manifest_is_refused_by_the_same_sentence ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

### `cargo test -p blastit --locked`

```console
$ cargo test -p blastit --locked
PKG_EXIT:0
     Running unittests src/lib.rs
test result: ok. 167 passed; 0 failed; 1 ignored; ...
     Running tests/critical_php_backend.rs
test result: ok. 4 passed; 0 failed; 0 ignored; ...
     Running tests/no_python_build.rs
test result: ok. 3 passed; 0 failed; 0 ignored; ...
     Running tests/smoke.rs
test result: ok. 31 passed; 0 failed; 0 ignored; ...
```

### `cargo test -p blastit --no-default-features --features php --test no_python_build --locked`

```console
$ cargo test -p blastit --no-default-features --features php --test no_python_build --locked
NOPY_EXIT:0
running 3 tests
test a_php_only_manifest_is_refused_by_the_interim_php_gate ... ok
test a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name ... ok
test a_wsgi_manifest_is_refused_by_the_capability_gate ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

### `make check`

```console
$ make check
CHECK_EXIT:0
...
cargo test -p blastit --features python --test critical_php_backend
running 4 tests
test check_refuses_a_php_route_before_anything_is_served ... ok
test a_php_route_is_refused_at_boot_and_never_reaches_a_wsgi_view ... ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test a_static_and_php_manifest_is_refused_by_the_same_sentence ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
...
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

`cargo fmt --all -- --check` is clean in the final state.

### Mutation proof (comment the `dev` call site → protected suite red)

```console
$ # the `if let Some(message) = php_backend_unavailable(&manifest) { dev_fail(..) }`
$ # block in run_dev is commented out, then:
$ cargo test -p blastit --test critical_php_backend --locked
MUTATED_EXIT:101
test a_php_route_is_refused_at_boot_and_never_reaches_a_wsgi_view ... FAILED
test a_static_and_php_manifest_is_refused_by_the_same_sentence ... FAILED
test check_refuses_a_php_route_before_anything_is_served ... ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
...
assertion `left == right` failed: a startup error writes nothing on stdout (§1.1)
  left: "  blastit dev · single-uid simulation · 127.0.0.1:44563\n ... 1 route\n ... Address already in use (os error 98)\n"
 right: ""
test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

Restored byte-for-byte from a pre-mutation copy (`cmp` identical); the suite is
green again and the acceptance commands above were re-run on the restored tree.

## Protected-test guard

```console
$ uv run --no-project tools/critical_guard.py --base a04ad51 --node MB.3.28
critical-guard: a04ad51..working tree, 8 changed file(s)
node MB.3.28: paths ['crates/blastit/src/stage0.rs', 'crates/blastit/src/check.rs', 'crates/blastit/tests/critical_php_backend.rs', 'crates/blastit/tests/no_python_build.rs']
  no change to a protected critical test
EXIT:0
```

(The protected file is **added**; `no_python_build.rs` is not a protected
`critical_*` file. `check.rs` is a normal source path now authorized by the
node.)

## Follow-ups (out of this node's paths)

- `crates/blastit/src/stage1.rs` repeats `no_python_support` but not this guard.
  Stage 0 refuses first on both public boot paths, so no manifest reaches
  stage 1 with a `[route.php]` route; `MB.3.13.3` removes the guard anyway.
- `crates/blastit/src/php_view.rs`'s module doc ("Interim state: no caller …")
  could mention that boot and `check` now refuse such a manifest.
