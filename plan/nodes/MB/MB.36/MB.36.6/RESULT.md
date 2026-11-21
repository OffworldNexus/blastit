# RESULT — MB.36.6 (attempt 1/3)

Base: `b42a6c9` (`[MB.36.5]`), branch `release/0.2.0`, worktree
`.worktrees/release-0.2.0`. Every command below ran in that worktree with
`PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"`.

Status: **done**. `make check` is EXIT=0, the archive is built with
`php,static` **and proved to serve a `static` mount**, and the no-default
corner compiles (`--all-targets`) and refuses a mount with
`SchedulerError::StaticNotCompiled`.

## 1. The feature matrix

### Standalone defaults and the python feature

```console
$ grep -q '^default = \["php", "static"\]' crates/blastit/Cargo.toml && echo "standalone default"
standalone default
$ grep -q 'python = \["dep:pyo3", "dep:blastit-view-python"\]' crates/blastit/Cargo.toml && echo "python feature"
python feature
$ test -z "$(grep -rn 'blastit-view/python\|blastit-view/php' crates/*/Cargo.toml)" && echo "stale feature edges gone"
stale feature edges gone
```

### `cargo tree` — the default and the feature-less configuration

```console
$ cargo tree -p blastit --locked -e normal --prefix none | grep -E '^(blastit|blastit-scheduler|blastit-view|blastit-view-(php|python|static)|pyo3|blastit-php-sapi) '
blastit v0.2.0 (…/crates/blastit)
blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
blastit-view v0.2.0 (…/crates/blastit-view)
blastit-view-static v0.2.0 (…/crates/blastit-view-static)
blastit-view v0.2.0 (…/crates/blastit-view) (*)
blastit-view v0.2.0 (…/crates/blastit-view) (*)
blastit-view-php v0.2.0 (…/crates/blastit-view-php)
blastit-php-sapi v0.2.0 (…/crates/blastit-php-sapi)
blastit-view v0.2.0 (…/crates/blastit-view) (*)
# php + static, no python/pyo3/view-python

$ cargo tree -p blastit --locked --no-default-features -e normal --prefix none | grep -E '^(blastit|blastit-scheduler|blastit-view|blastit-view-(php|python|static)|pyo3|blastit-php-sapi) '
blastit v0.2.0 (…/crates/blastit)
blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
blastit-view v0.2.0 (…/crates/blastit-view)
blastit-view v0.2.0 (…/crates/blastit-view) (*)
# no view-php, no view-static, no pyo3
```

The wheel configuration is additive on top of the defaults:

```console
$ cargo tree -p blastit-py --locked -e normal --prefix none | grep -E '^(blastit-py|blastit|blastit-view-(php|python|static)|pyo3) '
blastit-py v0.2.0 (…/crates/blastit-py)
blastit v0.2.0 (…/crates/blastit)
blastit-view-static v0.2.0 (…/crates/blastit-view-static)
blastit-view-php v0.2.0 (…/crates/blastit-view-php)
blastit-view-python v0.2.0 (…/crates/blastit-view-python)
pyo3 v0.30.1        # (three unified mentions)
# python + php + static
$ cargo check -p blastit-py --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.77s
```

### Wheel tag / `DT_NEEDED`

```console
$ uv run --no-project scripts/check_wheel_tag.py --max-glibc 2_28 dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: Tag: cp314-cp314-manylinux_2_28_x86_64
$ cargo build -p blastit-py --locked --features extension-module
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.58s
$ scripts/check-portable-elf.sh --allow-origin-rpath target/debug/lib_blastit.so
ok   target/debug/lib_blastit.so: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
# no libpython, no libphp.
```

### `make rs-no-python` (the new meaning of the old no-op line)

`cargo check -p blastit-view --no-default-features` is gone: `blastit-view`
has no features, so the line proved nothing. It is replaced by a
`grep -c pyo3|blastit-php-sapi crates/blastit-view/Cargo.toml` guard (the ELF
and `cargo tree` checks cannot see `[dev-dependencies]`), by
`cargo test -p blastit-view --locked` (its 43 unit cases) and by
`clippy -p blastit-view --lib`.

```console
$ make rs-no-python
ok   …/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
blastit-view: 0 pyo3 / blastit-php-sapi lines
test result: ok. 43 passed; 0 failed; …        # blastit-view (transport)
test result: ok. 242 passed; 0 failed; …       # blastit --lib
test result: ok. 3 passed; 0 failed; …         # no_python_build
default build: 0 pyo3 dependencies (no libpython linked)
EXIT=0
```

### `make rs-no-default` (new; wired into `make check`)

```console
$ make rs-no-default
cargo build -p blastit --locked --no-default-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.35s
cargo clippy -p blastit --locked --no-default-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.89s
no-default build: 0 blastit-view-php dependencies
no-default build: 0 blastit-view-static dependencies
no-default build: 0 pyo3 dependencies
cargo test  -p blastit-scheduler --no-default-features --test static_feature_off
running 2 tests
test a_manifest_with_a_mount_is_refused_before_any_channel_is_served ... ok
test the_same_boot_without_a_mount_is_not_refused ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
EXIT=0
```

The `--all-targets` part is the `critical_php_embed` half the dispatch asked
for: that suite now carries `#![cfg(feature = "php")]` (already present at the
base), and the feature-less build compiles it to an empty harness instead of
calling a `php`-gated item:

```console
$ cargo test -p blastit --locked --no-default-features --test critical_php_embed -- --list
0 tests, 0 benchmarks
```

As an extra end-to-end witness, the feature-less `target/debug/blastit dev` on
the same throwaway `static` mount also refuses the boot (the scheduler's
variant/sentence is pinned by `static_feature_off` above; `dev` only surfaces
the closed control channel):

```console
$ cargo build -p blastit --locked --no-default-features
$ BLASTIT_MANIFEST=…/Blastfile target/debug/blastit dev --port 19003
INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
INFO  runtime     static  serving static assets only
INFO  ready       serving  routes=0  workers=16
ERROR app.output  stderr  ctl.chan: the scheduler closed the channel
EXIT=1
```

(The default binary on the same root serves `200` — the archive proof below.)

## 2. The archive's capability, not just its build

`scripts/build-standalone.sh` now spells `php,static` at **all five** sites
(header comment, echoed command, the musl and gnu `cargo build` lines, and the
`cargo tree` pyo3 guard); `scripts/build-standalone-in-image.sh` and
`.github/workflows/publish-pypi.yml` document that the spelling is that one
script's (they delegate the build; the workflow comment previously promised
`--features php`).

```console
$ make standalone
build-standalone: cargo build --release --no-default-features --features php,static
    Finished `release` profile [optimized] target(s) in 49.66s
ok   …/target/release/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-standalone: glibc floor 2.39 (readelf --version-info)
build-standalone: 0 pyo3 dependencies (no libpython linked)
build-standalone: blastit-view-static present (a static mount is served)
build-standalone: 1 shim(s) for x86_64-gnu from …/target/php-sapi
build-standalone: 5249445 bytes -> dist/blastit-x86_64-unknown-linux-gnu.tar.gz
build-standalone: checksum -> dist/SHA256SUMS
EXIT=0
$ tar -tzf dist/blastit-*.tar.gz >/dev/null && echo "archive built"
archive built
```

The `cargo tree` guard now asserts **both** directions on the same graph —
`0 pyo3` (no `libpython`) and the positive `blastit-view-static present`.
Proved red by removing `static` from the spelling once and restoring the file
(`cmp` identical):

```console
$ sed -i 's/--features php,static/--features php/g' scripts/build-standalone.sh
$ scripts/build-standalone.sh
… Finished `release` profile …
build-standalone: 0 pyo3 dependencies (no libpython linked)
error: this archive cannot serve a `static` mount: blastit-view-static is not in its dependency graph
  fix: name every capability the artifact needs — this build is
  `--no-default-features --features php`, and a dropped `static` is a boot refusal
  (`SchedulerError::StaticNotCompiled`) a green build cannot see
EXIT=1
$ cp /tmp/opencode/mb366-build-standalone.good scripts/build-standalone.sh
$ cmp scripts/build-standalone.sh /tmp/opencode/mb366-build-standalone.good && echo "restored identical"
restored identical
$ make standalone
build-standalone: blastit-view-static present (a static mount is served)
EXIT=0
```

The archive's own `README.md` says `php` and `static` and points at `ADR-0016`
decision 6 (it previously said "the `php` capability" and named no `static`).

### The refusal is the privilege/`dev` one, not `StaticNotCompiled`

Unpacked archive, a throwaway root with one `static "/site" { root "public" }`
mount and `public/index.html`, run as uid 1000:

```console
$ cd /tmp/opencode/mb366-root && BLASTIT_MANIFEST=…/Blastfile …/blastit
{"…","event":"boot","component":"init","stage":"stage0","seccomp":true,…}
{"…","line":"init runs as uid 1000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid."…}
{"…","line":"  … a *dedicated copy* of the binary … setcap 'cap_chown,…,cap_setpcap+ep' <the copy>"…}
{"…","line":"  To run the same tree locally without root, use `blastit dev`; it is a single-uid simulation, not a sandbox"…}
EXIT=1
```

No `StaticNotCompiled` in the transcript. Second witness:

```console
$ strings …/blastit | grep -m5 -n 'blastit-view-static'
3798:crates/blastit-view-static/src/static_response.rs
```

Third, stronger witness — the archive **serves** the mount (same unpacked
binary, `blastit dev`, no privilege, throwaway root):

```console
$ BLASTIT_MANIFEST=…/Blastfile …/blastit dev --port 18998
INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
INFO  runtime     static  serving static assets only
INFO  ready       serving  routes=0  workers=16
INFO  request     GET /site/index.html → (unmatched)  200  0.6ms
$ curl -sS -D- http://127.0.0.1:18998/site/index.html
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 21
ETag: "15-18e92ec031e29ca0"
Last-Modified: Wed, 18 Nov 2026 15:55:29 GMT
X-Content-Type-Options: nosniff
Cache-Control: no-cache

<!doctype html>hello
```

### `check-portable-elf.sh` now refuses `libphp` too

A real negative witness (a shared object linked `--no-as-needed` against the
host's `libphp`):

```console
$ readelf -d …/libphp_probe.so | grep NEEDED
 0x… (NEEDED) Shared library: [libphp.so]
 0x… (NEEDED) Shared library: [libc.so.6]
$ scripts/check-portable-elf.sh …/libphp_probe.so
error: …/libphp_probe.so links libphp, so it carries its own PHP instead of loading the host's:
   0x… (NEEDED) Shared library: [libphp.so]
  fix: build without a crate that links it; a view dlopens libphp (ADR-0010 §1),
  and the standalone archive is built by scripts/build-standalone.sh.
EXIT=1
```

The positive side is the `ok` line from `rs-no-python`/`make standalone`
above (no `libpython`, no `libphp`, no `RPATH`/`RUNPATH`).

## 3. The living spec

```console
$ for c in blastit-view-python blastit-view-php blastit-view-static; do grep -q "$c" ARCHITECTURE.md && echo "architecture names $c"; done
architecture names blastit-view-python
architecture names blastit-view-php
architecture names blastit-view-static
$ test -z "$(grep -rn 'WorkerRuntime' ARCHITECTURE.md crates/)" && echo "no trait claim left"
no trait claim left
```

- `ARCHITECTURE.md`: §2 intro + §2.4 name `blastit-view-static` and say the
  scheduler only admits/dispatches; §2.4's answer sentence is the true one — the
  head/body leave as `blastit-frame`'s `RespHead`/`RespBody` through the
  scheduler's own `http.chan` writer (`Outbound`/`http_push`), and `blastit-view`
  reaches the scheduler only as the re-exported request-target decoder; §2.5
  names `blastit-view-python` and the transport; §2.6 replaces
  `WorkerRuntime`/`blastit-view`'s `PhpEmbedRuntime`
  with `blastit-view-php`'s concrete runtime + `discovery`/`pinning`/`probe`;
  §2.7's crate table gains `blastit-view-python`, `blastit-view-php`,
  `blastit-view-static` and rewrites the `blastit`/`blastit-scheduler`/
  `blastit-view` rows (there was a §2.7 table even though §8.5 says "no
  separate crate-map section"); §3.1 attributes the codec/flow/decoder;
  §4 attributes the fork to `zygote`/`spawn` and the child order to
  `blastit_sys::child::enter`; §7.3's `setpgid(0,0)` sentence now says it runs
  inside `enter` between `prepare` and the filter; §9.3 item 12 names
  `blastit_view_python::serve` and `blastit-view-python`'s `zygote`.
- `docs/knowledge/LESSONS.md`: the `MB.17` feature-unification bullet now says
  `blastit-view` has **no** features (`MB.36.5`) so no default can re-add
  `python`; a new bullet records the archive's `--no-default-features` trap and
  the feature-off `unused_mut` fix.
- `ADR-0010`: Status is `Accepted (superseded in part by ADR-0016: the
  distribution command spelling — the archive is built with --features
  php,static and the wheel carries python, php, static)`; the body is not
  rewritten. `docs/decisions/README.md`'s ADR-0010 row matches.
- `ADR-0007` and `docs/design/static-and-spa.md`: the six
  `crates/blastit-scheduler/src/{static_asset,static_response,rewrite}.rs`
  occurrences and the two moved static critical-suite paths now name
  `crates/blastit-view-static/…`; the mutation audit is
  `make mutants-rust PKG=blastit-view-static` (the three `-f` modules are in
  that crate, not the scheduler).
- `docs/design/backend-runtimes.md`: header line "Trait contract superseded by
  ADR-0016; the provider taxonomy stands"; §4.7's standalone/container commands
  are `--features php,static`, the wheel features `python, php, static`, and the
  two other stale `--features php` requirements (§2 R9, §7.1) follow.
- `docs/design/view-crates-architecture.md`: §4.2.4's `static_asset.rs` row adds
  `StaticRoot::with_max_bytes`; §4.4 D1's census is corrected to **one**
  `Privileges::Shared` (stage 1's dev branch only; `check` constructs neither
  and boots no stage 1) and **one** `Filter::Off`, verified by
  `grep -rn -e 'Privileges::Shared' -e 'Filter::Off' crates/blastit/src
  crates/blastit-view/src crates/blastit-view-*/src` → `stage1.rs:2345` and
  `stage1.rs:2357` only.

## 4. The deferred refusal sentence (`crates/blastit-view-php/src/discovery.rs`)

The claim noun now travels to the callback instead of being hard-coded as
`"the file"` in `stage0::object_is_protected`:

```console
-    pub is_protected_object: &'a dyn Fn(&Path) -> Result<(), String>,
+    pub is_protected_object: &'a dyn Fn(&str, &Path) -> Result<(), String>,
…
-        if let Err(detail) = (deployment.is_protected_object)(path) {
-            return Some(present_refusal(&prefix, &format!("{what}: {detail}"), …));
+        if let Err(detail) = (deployment.is_protected_object)(what, path) {
+            return Some(present_refusal(&prefix, &detail, …));
```

`Provenance`'s own sentence is `"{what} ({path}) could be replaced by another
uid: …"`, so the refusal now reads once:
`route "app": entrypoint php "public/index.php": the NTS libphp (/…/libphp8.4.so) could be replaced by another uid: …`.

The 27-case module is unchanged except for the callback types:

```console
$ cargo test -p blastit --locked --lib php_boot
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 215 filtered out; finished in 0.22s
```

## 5. Acceptance tails

```console
$ cargo build -p blastit --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
$ make platform-matrix-check
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; … 0 leg(s) dropped
$ cargo metadata --offline --locked --format-version 1 >/dev/null
EXIT=0        # Cargo.lock unchanged (no dependency edit)
$ cargo fmt --all -- --check
EXIT=0
$ make docs-check docs-test doc-tone
48 fences: 30 executed, 18 accounted, 0 failed · 38.0s
doc-tone: ok — 30 user-facing page(s), no banned phrase
EXIT=0
$ make critical-guard NODE=MB.36.6
critical-guard: HEAD..working tree, 22 changed file(s)
node MB.36.6: paths ['crates/blastit/Cargo.toml', …]
  no change to a protected critical test
EXIT=0
$ make check
… (all targets) …
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`make check` log: `/tmp/opencode/mb366-check-final2.log` (EXIT=0). It includes
`rs-no-python`, `rs-no-default`, `test-critical`, `rs-test`, `docs-*`,
`platform-matrix-check`, `workflow-check`, `packaging-gates-test`,
`critical-guard`, `plan-validate`.

## 6. Scope / state notes

- Files changed are exactly the node's `paths` (plus `LESSONS.md`, listed) —
  no `critical_*` test was edited, no `#[allow]` added, no `unsafe`.
- `README.md` was already dirty at dispatch and was never touched, staged or
  restored. `docs/design/portable-lsp.review-{ai-ergonomics,dx}.md` are another
  node's untracked evidence and were left alone.
- `plan/BUDGET.md` changed at 15:24 while this session was running (its
  `updated:` timestamp only); it is not one of this node's files and was not
  edited here.

## Follow-ups

- `scripts/build-wheel.sh`'s comment still says "wheels are `python, php`" and
  "`blastit`'s default features are `php` only"; the actual command it runs is
  correct (file outside this node's `paths`).
- `docs/design/backend-runtimes.md` §7.1 names
  `crates/blastit-view/tests/critical_view_transport.rs` and
  `crates/blastit/tests/critical_backend_capability.rs`, and neither file is in
  this tree — stale paths in an accepted design doc, not a build claim
  (`cargo test --workspace` is green).
- Two dependency comments still say "`blastit-view` still defaults `python`",
  false since `MB.36.5`: `crates/blastit-scheduler/Cargo.toml:29` and
  `crates/blastit-view-static/Cargo.toml:17`. The `default-features = false`
  edges themselves remain correct and are kept.
