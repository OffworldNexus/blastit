---
id: MB.57
title: "Fix 4 post-MB.56 CI items (build-wheel.sh quote, py-shim clippy cfg, e2e_dev php-only cfg & root-only Cargo JSON executable lookup)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "scripts/build-wheel.sh"
  - "crates/blastit-py-shim/**"
  - "crates/blastit/tests/**"
  - .github/workflows/ci.yml
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T23:33"
updated: "2026-11-20T23:40"
---

# Goal
Close the 4 tiny CI regressions on PR #64 (`351021b` after `MB.56` deleted 149k lines):

1. **`scripts/build-wheel.sh:438` (`ci.yml` `py3-none wheel` & `publish-pypi.yml` `wheels`):**
   - Line 438 `# check. make py-sapi asserts each shim's own artifact contract` introduced an apostrophe inside `bash -euo pipefail -c '...'` (opened at line 409), swallowing `else` at line 451! Remove any apostrophes inside that single-quoted `bash -c '...'` block, check `PY_SAPI_PROBE=0` if `Makefile:py-sapi` still runs a `dlopen` probe (`manylinux_2_28` has no shared `libpython`), and prove `MATURIN="uvx --offline maturin" scripts/build-wheel.sh --require-shims py --out /tmp/opencode/wheel-check` builds the repacked `py3-none` wheel cleanly!

2. **`crates/blastit-py-shim` (`py-sapi.yml` `lint`):**
   - Gate `mod request;` / `mod buffer;` (or unused items when `extension-module` is off) under `#[cfg(feature = "extension-module")]` so BOTH `cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings` AND `cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings` pass with 0 warnings!

3. **`crates/blastit/tests/e2e_dev.rs:1142` (`php-sapi.yml` & `publish-pypi.yml` `shims`):**
   - `cargo test --locked -p blastit --test e2e_dev --no-default-features --features php,static --no-run` failed at `e2e_dev.rs:1142` (`E0425` around `PARALLEL_APP` feature-gating). Fix the `#[cfg(feature = "python")]` gating across `crates/blastit/tests/*.rs` and prove `--no-default-features --features php,static --no-run` AND `--no-default-features --no-run` compile cleanly with `-D warnings`!

4. **`.github/workflows/ci.yml` `root-only` job (`find: ‘target/debug/deps’: No such file or directory`):**
   - Rust 1.100 places test binaries under `target/debug/build/blastit/.../out/e2e_cli-*`, not `target/debug/deps`. Resolve the test binary path via `cargo test -p blastit --test e2e_cli --locked --no-run --message-format=json` (`reason == "compiler-artifact"` and `profile.test == True`)!

# Acceptance
- `MATURIN="uvx --offline maturin" scripts/build-wheel.sh --require-shims py --out /tmp/opencode/wheel-check` produces `/tmp/opencode/wheel-check/blastit-0.2.0-py3-none-*.whl` (clean up `/tmp/opencode/wheel-check` after)
- `cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings` && `cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings`
- `cargo clippy -p blastit --tests --locked --no-default-features --features php,static -- -D warnings`
- `make fast-check`
