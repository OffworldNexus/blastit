# MB.57 — RESULT

STATUS: done

## 1. `scripts/build-wheel.sh` apostrophe in the single-quoted `bash -c` block
Rewrote the comment at ~437 with no apostrophe ("asserts the artifact contract of each
shim"). No behaviour change; the `else` at ~451 is back inside the `if`.

Evidence:
```
$ bash -n scripts/build-wheel.sh && echo OK
OK
$ sed -n '410,449p' scripts/build-wheel.sh | <substitute '"${var}"'> | bash -n
inner bash -n OK   # zero `'` left in the inner docker script
$ MATURIN="uvx --offline maturin" scripts/build-wheel.sh --require-shims py --out /tmp/opencode/wheel-check | tail
📦 Built wheel to /tmp/opencode/wheel-check/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
build-wheel: staged 3 shim(s) from this host for x86_64-gnu
repack: blastit/_bin/ carries blastit, blastit-lsp, libblastit_php84-x86_64-gnu.so, libblastit_py313-x86_64-gnu.so, libblastit_py314-x86_64-gnu.so
ok   .../_bin4: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
$ ls /tmp/opencode/wheel-check/
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
$ rm -rf /tmp/opencode/wheel-check
```
(The host path ran; the `--manylinux` container branch is covered by `bash -n` plus the
inner-block parse above, not by a container build here.)

## 2. `blastit-py-shim` clippy, both feature shapes
`mod buffer;` / `mod request;` are now gated exactly like `mod surface` — they are reached
only through it, and a `test` + `extension-module` build compiles `surface` out.
```
$ cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings | tail -2
    Checking blastit-view-python v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.25s
$ cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings | tail -1
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.10s
```
Zero warnings in both.

## 3. `crates/blastit/tests/*` python-feature leaks
Three `#[cfg(...)]` attributes had been glued onto the tail of a `///` doc line, so the item
they were meant to gate was compiled unconditionally:
* `PHP_GRANT_MANIFEST` (`#[cfg(feature = "php")]`),
* `a_prefix_mount_serves_under_its_prefix_and_the_longest_one_wins` (`static`),
* `the_banner_names_every_view_and_mount` (`all(python, static)`) — the E0425 on `PARALLEL_APP`.

Also fixed the real class of leak: `Boot`/`Swap`/`assert_never_served`/`spawn_reader`/
`sidecar`/`REQUEST` and their imports in `e2e_prod_refusals.rs` were gated `python` although
the PHP legs boot the same collector — now `any(python, php)`; the serialized-control
constants and `Dev::json_request` in `e2e_dev.rs` (used only by the WSGI legs) are `python`.
```
$ cargo clippy -p blastit --tests --locked --no-default-features --features php,static -- -D warnings | tail -2
    Checking blastit v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
$ cargo clippy -p blastit --tests --locked --no-default-features -- -D warnings | tail -2
    Checking blastit v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
```

## 4. `.github/workflows/ci.yml` `root-only` executable lookup
The `find target/debug/deps` guess is gone: the step now resolves the harness from
`cargo test ... --no-run --message-format=json` through a stdlib-python filter
(`reason == "compiler-artifact"`, `profile.test`, non-empty `executable`) using the job's own
`setup-python` interpreter (no `uv` in that job). The `::error::` + `exit 1` guard is kept.
Filter exercised locally with two sample JSON lines:
```
$ printf ... | uv run --no-project python -c '<the step>'
/tmp/target/debug/build/x/out/e2e_cli-abc
```

## Gate
`cargo fmt --all -- --check` clean; `make fast-check` run after this file was written.
