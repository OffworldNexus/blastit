# Result MB.3.17

## Summary

The external FastCGI path is gone from the code: `crates/blastit-fastcgi` (3 495 lines, 7 files)
is deleted with `Cargo.lock`, `Executor::Pool`/`FastCgiPolicy`/`Route::fastcgi` and the whole
parse/validate family (`FastCgiPolicy`, `FastCgiEntryInvalid`, `FastCgiListenDerived`,
`FastCgiWorkersInvalid`, `FastCgiDeadDeclaration`, `check_fastcgi*`) are removed, and
`BLASTIT_ALLOW_EXTERNAL_FASTCGI` / `BLASTIT_PHP_FPM` are gone from code and
`docs/reference/settings.md`. `[route.fastcgi]` is now an **unknown key that names itself**
(`route[0].fastcgi: unknown key`), with the accepted twin asserted in the same test. The
protected suites under `crates/blastit-fastcgi/tests/critical_*.rs` were deleted **with
`critical_guard.py --base 9889a29 --node MB.3.17` as authorizer** (verdict below).

**Attempt 2 lands the one thing that blocked `make check`:** `docs/frameworks/php.md` cannot be
edited here (`docs/frameworks/**` is `MB.12`'s), so `crates/blastit-manifest/tests/docs_fences.rs`
now carries a one-row **accounting table** (`STALE`) that asserts that page's fence *still fails,
for the stated reason* (`fastcgi: unknown key`), plus a self-clearing test that goes red the
moment the page boots. `make check` is **exit 0**.

## Evidence

### 1. `cargo build --locked` — exit 0

```console
$ cargo build --locked; echo "build-exit=$?"
   Compiling blastit-view v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit-view)
   Compiling blastit v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit)
   Compiling blastit-py v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.86s
build-exit=0
```

The crate is gone from the workspace (members are the glob `crates/*`, so no `Cargo.toml` edit
was needed; `Cargo.lock` lost its `[[package]] blastit-fastcgi` stanza):

```console
$ cargo metadata --no-deps --format-version 1 | grep -c blastit-fastcgi
0
$ cargo tree -p blastit-fastcgi
error: package ID specification `blastit-fastcgi` did not match any packages
```

### 2. `make check` — **exit 0**

```console
$ make check; echo "check-exit=$?"
...
clippy      -p blastit{,-view} --no-default-features
cargo clippy -p blastit --no-default-features --features php --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
interpreter-free build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
check-exit=0
```

111 `test result: ok` lines, zero `FAILED`, zero `Error 1`: `rs-fmt`, `rs-lint`, `rs-doc`,
`rs-test`, `test-critical` (`Ran 50 tests` / `OK`), `py-lint` (`All checks passed!`),
`docs-unit` (`63 passed …`), `docs-check` (`No issues found`), `docs-test`
(`56 fences: 29 executed, 27 accounted, 0 failed`), `rs-no-python`
(`interpreter-free build: 0 pyo3 dependencies (no libpython linked)`), `plan-validate`
(`plan OK`). `critical-guard` prints its report-only note about the two authorized deletions
(§7); it is not a failure.

For attempt 1's record, this is what the same command returned *before* the accounting table
existed — one failure, one test, one page:

```console
test every_documented_manifest_parses_and_validates ... FAILED
docs/frameworks/php.md does not boot:
manifest has 1 error(s)
  - route[0].fastcgi: unknown key
make: *** [Makefile:126: rs-test] Error 101
```

### 2b. The `docs_fences` accounting mechanism (attempt 2)

`docs/frameworks/php.md` teaches `[route.fastcgi]`, a key this node retired, and
`docs/frameworks/**` is `MB.12`'s path while its lenses run, so the code half may not rewrite
it. `crates/blastit-manifest/tests/docs_fences.rs` therefore gains a one-row table rather than a
skip:

```rust
const STALE: [(&str, &str, &str); 1] =
    [("docs/frameworks/php.md", "fastcgi: unknown key", "MB.3.20")];
```

- `every_documented_manifest_parses_and_validates` now branches on it: an accounted page must
  **fail**, and its error text must contain `fastcgi: unknown key`; every other page must boot
  and carry a route. There is no path through the suite where an accounted page is simply
  ignored.
- `the_stale_accounting_names_only_manifests_that_still_fail` asserts (a) every `STALE` path is
  in `FILES` and (b) no `STALE` path boots, with the same needle. It is the self-clearing half:
  the moment `MB.3.20` (or `MB.3.5.4`) retargets the fence, this test goes red until someone
  deletes the entry.
- The `STALE` doc comment records the whole reason: the page teaches a key the parser refuses,
  the page is `MB.3.20`'s to rewrite **and `MB.3.20` is responsible for removing the entry**,
  and `[route.php]` cannot be substituted before `MB.3.5.4` because it does not parse yet (it
  reads as `grant on unknown proxy "php"`).

Both halves were proved red, then restored:

```console
# (1) a wrong needle — the reason must match, not just "some error"
$ sed -i 's/"fastcgi: unknown key"/"not a key this build knows"/' …/docs_fences.rs
test the_stale_accounting_names_only_manifests_that_still_fail ... FAILED
  docs/frameworks/php.md is accounted stale for "not a key this build knows" (MB.3.20), but it fails differently:
test every_documented_manifest_parses_and_validates ... FAILED
  docs/frameworks/php.md is accounted stale for "not a key this build knows" (MB.3.20), but it fails differently:
test result: FAILED. 4 passed; 2 failed

# (2) an entry on a page that boots — the accounting cannot outlive the rewrite
$ sed -i 's|("docs/frameworks/php.md"|("docs/local.md"|' …/docs_fences.rs
test every_documented_manifest_parses_and_validates ... FAILED
  docs/local.md boots again: delete its `STALE` entry — MB.3.20 owns the rewrite and the accounting must not outlive it
test the_stale_accounting_names_only_manifests_that_still_fail ... FAILED
  docs/local.md boots: delete its `STALE` entry — MB.3.20 owns the rewrite and the accounting must not outlive it
test result: FAILED. 4 passed; 2 failed

# restored, formatted, green again
$ cargo fmt --all -- --check && cargo test -p blastit-manifest --test docs_fences --locked
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 3. Orphan-reference grep

```console
$ grep -rn 'fastcgi\|FastCgi\|ALLOW_EXTERNAL' crates/ scripts/ Makefile .github/ | wc -l
17
$ … | awk -F: '{print $1}' | sort | uniq -c
      1 crates/blastit-manifest/src/lib.rs
      3 crates/blastit-manifest/src/model.rs
      3 crates/blastit-manifest/src/parse.rs
      2 crates/blastit-manifest/tests/docs_fences.rs
      5 crates/blastit-manifest/tests/parse.rs
      3 crates/blastit/tests/no_python_build.rs
```

**The grep cannot be empty, by construction:** the retired key has to be *spelled* somewhere in
order to be refused *by name* (`NODE.md` requirement, LOG 15:57). Every surviving hit falls in
two classes and no third:

| File | Lines | Why it must exist |
|---|---|---|
| `crates/blastit-manifest/src/parse.rs` | 36, 41, 43 (`const RETIRED_ROUTE_KEYS: &[&str] = &["fastcgi"];`) | the retired-key list that produces `route[0].fastcgi: unknown key` |
| `crates/blastit-manifest/src/{lib,model}.rs` | doc comments + `assert!(!ALL_KINDS.contains(&"fastcgi"))` | says in words that the kind and the key are gone |
| `crates/blastit-manifest/tests/parse.rs` | the new `fastcgi_is_an_unknown_route_key_and_the_static_twin_parses` | the refusal test itself |
| `crates/blastit/tests/docs_fences.rs` | 38, 55 (`("docs/frameworks/php.md", "fastcgi: unknown key", "MB.3.20")`) | the accounting row's needle is the refusal it asserts, and the comment names the deleted key |
| `crates/blastit/tests/no_python_build.rs` | 50, 145, 153 | the Python-free build's refusal test (§4) |

`scripts/`, `Makefile` and `.github/` have **zero** hits for this pattern. The case-insensitive
grep adds only `crates/blastit-php-sapi/**` (7 hits: "refuse the `PHP_VALUE`/`PHP_ADMIN_VALUE`
names that exist in the FastCGI world, `CVE-2012-1823`") and `Makefile:376` ("the shim …
refuses the four FastCGI names") — the CVE surface the shim must keep refusing, in a crate
outside this node's `paths`. `ALLOW_EXTERNAL` has **zero** hits in `crates/ scripts/ Makefile
.github/ python/ e2e/` (`grep-exit=1`).

### 4. `cargo test -p blastit --locked` — exit 0

```console
$ cargo test -p blastit --locked
test result: ok. 183 passed; 0 failed; 1 ignored; …  (lib)
test result: ok. 4 passed; 0 failed; …             (audit_group_hint)
test result: ok. 19 passed; 0 failed; …            (critical_check_engine)
test result: ok. 12 passed; 0 failed; …            (critical_config_and_presets)
test result: ok. 19 passed; 0 failed; …            (critical_dev_and_fail_closed)
test result: ok. 12 passed; 0 failed; …            (critical_static_cli)
test result: ok. 1 passed; 0 failed; …             (dev_signal_reap)
test result: ok. 0 passed; 0 failed; …             (no_python_build: empty in the wheel build, by design)
test result: ok. 31 passed; 0 failed; …            (smoke)
test result: ok. 17 passed; 0 failed; …            (view/flow)
```

The Python-free build still refuses **both** WSGI and FastCGI at exit 1 — `make rs-no-python`:

```console
$ make rs-no-python
running 2 tests
test a_wsgi_manifest_is_refused_by_the_capability_gate ... ok
test a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name ... ok
test result: ok. 2 passed; 0 failed; … finished in 0.02s
```

`crates/blastit/tests/no_python_build.rs` **was** updated, because its FastCGI row asserted a
sentence that no longer exists (`MB.3.13's wiring`, `BLASTIT_ALLOW_EXTERNAL_FASTCGI`): the
constant is now `RETIRED_EXTERNAL_PHP`, the test is
`a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name`, and it asserts exit
`1`, empty stdout, `route[0].fastcgi: unknown key`, and the *absence* of both
`install the Python package: uv add blastit` and `BLASTIT_APP: the WSGI callable …` — so the
refusal is the parser's and not the capability gate passing by accident. It is still a
full-process test of the artifact (`CARGO_BIN_EXE_blastit`), not a unit call.

### 5. `cargo test -p blastit-manifest --locked` — exit 0

```console
$ cargo test -p blastit-manifest --locked
unittests src/lib.rs            → ok 40 passed 0 failed
tests/critical_manifest_011.rs  → ok 17 passed 0 failed
tests/critical_manifest_edit.rs → ok 23 passed 0 failed
tests/critical_static_manifest.rs → ok 15 passed 0 failed
tests/docs_fences.rs            → ok  6 passed 0 failed   (the accounting row included)
tests/edit_branches.rs          → ok 19 passed 0 failed
tests/grants.rs                 → ok 13 passed 0 failed
tests/mutation_gaps.rs          → ok 20 passed 0 failed
tests/parse.rs                  → ok 34 passed 0 failed   (the refusal + twin)
tests/routing.rs                → ok 11 passed 0 failed
tests/uid.rs                    → ok  6 passed 0 failed
tests/validate.rs               → ok 29 passed 0 failed
```

The new `[route.fastcgi]` test, proven failing first (before any edit: it reported the manifest
*parsed*, `fastcgi: Some(FastCgiPolicy { entry: "public/index.php", … })`):

```console
$ cargo test -p blastit-manifest --test parse fastcgi_is_an_unknown
test fastcgi_is_an_unknown_route_key_and_the_static_twin_parses ... FAILED
  manifest should not parse: Manifest { … fastcgi: Some(FastCgiPolicy { entry: "public/index.php", workers: None }) … }
```

What a developer actually sees, from the wheel build (`blastit dev`, the artifact, not a unit
call):

```console
$ cd /tmp/opencode/mb317/fence && blastit dev --port 9099
blastit dev: manifest ./blastit.toml: manifest has 1 error(s)
  - route[0].fastcgi: unknown key
dev-exit=1
```

**Deviation, stated plainly:** the brief asked for the twin to be `[route.php]` *parsing*.
`[route.php]` does not parse on this base — `PhpPolicy`/`Executor::Php` are `MB.3.5.4`'s, still
`ready` — so asserting it would have been a failing test or a half-implementation of a sibling
node. The twin in the test is therefore the block that *is* accepted today, `[route.static]`,
and the test's doc comment says why. `MB.3.20` carries the `[route.php]` twin to the docs pass.

### 6. `make php-sapi-e2e` — exit 0 (MB.3.12's probe still passes)

```console
$ make php-sapi-e2e
$ uv run --no-project e2e/php_sapi_probe.py
php-sapi-e2e: readelf -d libblastit_php84.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.4 abi 1: status 200, cpu_ns 10030510 <= wall_ns 11944580, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.4 abi 1: status 200, cpu_ns 11427475 <= wall_ns 11956045, exit()+fatal contained, next request served
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.4 abi 1: status 200, cpu_ns 8504559 <= wall_ns 11839523, exit()+fatal contained, next request served
exit=0
```

### 7. `critical_guard` — the protected deletion is authorized

```console
$ uv run --no-project tools/critical_guard.py --base 9889a29 --node MB.3.17
critical-guard: 9889a29..working tree, 47 changed file(s)
node MB.3.17: paths ['crates/blastit-fastcgi/**', 'crates/blastit-manifest/**', …]
  D  crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs  [authorized by crates/blastit-fastcgi/**]
  D  crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs  [authorized by crates/blastit-fastcgi/**]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.3.17
guard-exit=0
```

The same command run *before* the deletion printed `no change to a protected critical test`, so
the authorization is about exactly these two files and nothing else. `git rm` (never a silent
`rm`) removed them. `make check`'s `critical-guard` target runs without `--node`, so it prints
them as `[VIOLATION: not covered by any node path] / report only: 2 breaking change(s)` — that
is the report-only mode, not a failure; the enforced run above is the authority. No protected
test was weakened and none outside `crates/blastit-fastcgi/**` was touched
(`git status --short` lists no other `critical_*` file).

## What was deleted and why

- **The crate was green, audited and still the wrong code for the architecture.** It passed its
  own `critical_fastcgi_*` suites, its mutant campaign (`mutants.out/`) and `make check` while
  implementing a transport `ADR-0010` §1 had already rejected. Every test proved the
  *implementation* right; none could prove the *decision* right. It bought a second threat model
  (the CGI/FastCGI param-injection class the manifest's param wall existed for), a second
  transport to keep green, and a switch (`BLASTIT_ALLOW_EXTERNAL_FASTCGI`) whose only purpose was
  to weaken the product. One PHP backend, one threat model, one CPU-accounting path.
- **No `Cargo.toml` edge existed:** nothing in the workspace depended on the crate, so removing
  it is a pure deletion; `members = ["crates/*"]` needed no edit and `Cargo.lock` lost one stanza.
- **Nothing to un-wire in the dispatcher:** `FastCgiUpstream`, `Ctl::Pool*`, a backend registry
  and a `FastCgi` executor arm were design-only (`grep -rn FastCgiUpstream crates/` is empty).
  What existed was the manifest surface — `Executor::Pool(_, FastCgiPolicy)` and five
  `ManifestError` variants — plus the stage-0 refusal. All removed; `Executor::View` stays a
  one-arm enum so the seam `MB.3.5.4` fills with `Php` is unchanged, and `ALL_KINDS` now says
  `["wsgi"]`.
- **`[route.fastcgi]` is retired, not repurposed:** `RETIRED_ROUTE_KEYS` is checked in both the
  route walk (one `UnknownKey` at `route[i].fastcgi`) and the R8 top-level walk (which lets the
  single diagnostic through instead of reporting fields inside a block that does not exist). The
  name stays listed so a proxy an author happens to call `fastcgi` cannot turn the block into an
  inline grant: the refusal is the same sentence wherever it is written.
- **`docs/reference/settings.md`:** the `BLASTIT_PHP_FPM` row (`the PHP Blastit runs for a
  [route.fastcgi] route`) is gone. `## Backends` is left as an empty heading for now *only*
  because `docs/frameworks/php.md` links `../reference/settings.md#backends` and
  `zensical --strict` fails on a dead anchor; `MB.3.20` fills it (`BLASTIT_LIBPHP`) or drops the
  heading and the link together. There was never a `BLASTIT_ALLOW_EXTERNAL_FASTCGI` row.

## Files

Deleted (7): `crates/blastit-fastcgi/{Cargo.toml,src/{lib,cgi,lockdown,param,record}.rs,tests/{critical_fastcgi_codec,critical_fastcgi_lockdown}.rs}`

Modified (13):

- `Cargo.lock` — one `[[package]] blastit-fastcgi` stanza removed
- `crates/blastit-manifest/src/model.rs` — `FastCgiPolicy` gone, `Executor::Pool` gone, `Route::fastcgi`/`with_fastcgi` gone, `is_all_static`/`can_boot_without_app` simplified, tests updated
- `crates/blastit-manifest/src/parse.rs` — `RETIRED_ROUTE_KEYS` + `UnknownKey`, `fastcgi_policy`/`workers` gone, four unit tests removed
- `crates/blastit-manifest/src/validate.rs` — `check_fastcgi{,_entry,_workers,_declarations}` and six pinned phrases gone, `check_static_declarations`' front-controller escape removed, six unit tests removed
- `crates/blastit-manifest/src/error.rs` — `FastCgiEntryInvalid`, `FastCgiListenDerived`, `FastCgiWorkersInvalid`, `FastCgiDeadDeclaration` gone
- `crates/blastit-manifest/src/lib.rs` — re-export and doctest updated (`ALL_KINDS == ["wsgi"]`)
- `crates/blastit-manifest/tests/parse.rs` — new refusal test + accepted twin
- `crates/blastit/src/stage0.rs` — the `[route.fastcgi]` stage-0 branch, its constant and its unit test removed
- `crates/blastit/tests/no_python_build.rs` — FastCGI row re-pointed at the parser's sentence
- `crates/blastit-manifest/tests/docs_fences.rs` — the `STALE` accounting table, the branch that
  asserts an accounted page fails for the stated reason, and the self-clearing test (§2b)
- `python/tests/runnable_docs/inventory.toml` — `docs/frameworks/php.md` fence 7's `reason` no
  longer says a "FastCGI worker" serves `public/index.php`; it names the embedded-PHP worker
  (`MB.3.13`/`MB.3.7`)
- `docs/reference/settings.md` — `BLASTIT_PHP_FPM` row removed
- `docs/knowledge/LESSONS.md` — one bullet: a green, audited, zero-dependency crate can still be the wrong code

`plan/nodes/MB/MB.3/MB.3.20/` — new proposed doc-writer node for the held docs pass.

## Follow-ups

1. **`MB.3.20` (created, `proposed`, `depends_on: MB.3.17, MB.12`) is responsible for removing
   the `STALE` entry** in `crates/blastit-manifest/tests/docs_fences.rs`, by name, as part of
   rewriting `docs/frameworks/php.md` onto the embedded PHP story (`[route.php]` + `libphp`).
   `the_stale_accounting_names_only_manifests_that_still_fail` fails from the moment that fence
   boots until the entry is deleted, so the exception cannot survive the rewrite or be inherited
   by the next release. `MB.3.20` also owns: the page's fallback note, the `#backends` anchor in
   `docs/reference/settings.md` (filled with `BLASTIT_LIBPHP` or dropped together with the link),
   the design/ADR supersession lines and `AUDIT-MB.3.4-gate.md`.
2. **`MB.3.5.4` is now half-superseded:** it still says "[route.php] (`PhpPolicy`,
   `Executor::Php`) **and** narrow `[route.fastcgi]` to external-only; drop
   `FastCgiPoolSocketInvalid`" — the second half is done here, and `FastCgiPoolSocketInvalid`
   never existed on disk. Re-scope it to `[route.php]` only, and have it carry the
   entry/workers/spa/CONNECT/TRACE rule tests that left with `check_fastcgi` (they transfer
   verbatim; `MB.3.5.4`'s `paths` already cover `tests/parse.rs`). When it lands, the `STALE`
   entry may be *replaced* by a booting fence — but only by `MB.3.20`'s rewrite, never by
   deleting the entry first.
3. **`python/tests/runnable_docs/inventory.toml`** — done here (the lead widened this node's
   `paths` for attempt 2): fence 7 of the PHP page no longer claims a "FastCGI worker";
   `MB.3.20` should only re-check it.
4. **Host tooling gotcha (not written to `LESSONS`, to keep the node to one bullet):** an
   editor-side formatter rewrote `crates/blastit/src/stage0.rs` with a pre-2024 *style edition*
   (ASCII-sorted use lists, `assert!(…,);` collapsed) on save, which `make rs-fmt` then rejected.
   `cargo fmt --all` restores it (LESSONS already warns about bare `rustfmt`, `MB.1.5`); worth
   re-checking after any `edit` call in this workspace.
