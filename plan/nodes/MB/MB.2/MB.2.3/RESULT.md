# MB.2.3 — Parse and validate `[route.static]` blocks in `blastit-manifest`

STATUS: done (manifest level; the CLI/boot half is `MB.2.6`)

## Summary

- **Grammar** (`R1`): `[route.static]` with exactly `root` (required string),
  `index` (string, default `index.html`), `spa` (bool, default `false`),
  `immutable` (array of strings, default `[]`). `static` is a reserved route
  key, so `[route.static] queries = […]` is `route[0].static.queries: unknown key`
  and a declared `[proxy.static]` can never be granted.
- **Model**: `StaticPolicy { root, index, spa, immutable }` (all fields public, as
  the design draws it), `Route::static_policy()`, and
  `Manifest::{is_all_static, static_roots, check_static_roots}`. Existence is an
  explicit call, never part of `validate`, so `docs_fences.rs` keeps parsing the
  page from a workspace root that has no `public/` — and the protected suite
  proves both halves (`validation_never_touches_the_filesystem`).
- **Five exact `ManifestError` variants** with the design's messages, all carrying
  `Location`: `StaticRootInvalid`, `StaticRootUnusable`, `StaticIndexInvalid`,
  `StaticImmutablePattern`, `StaticDeadDeclaration`. Fixed `rule`/`reason`
  phrases only; no `strerror` in any message.
- **Rules** (`R2`–`R5`): root empty/leading `/`/`..` segment/`\`/C0 control;
  index leading `/`, empty segment, `..`, dotfile segment, `\`, control byte;
  `immutable` `/`-anchored, `..`-free, control-free, ≤ 32 patterns, ≤ 256 bytes
  each; the four dead declarations (`[route.ingress]`, `prefork = true`, a
  `[route.<proxy>]` grant, a route-level `groups`) on a static route that can
  never reach the app (`spa = true` or no manifest `app`).
- **`check_static_roots`**: `metadata` (symlinks followed, so `current/public`
  works) → `is_dir` → `read_dir` whose first entry must not be an `Err`, mapping
  to `no such file or directory` / `not a directory` / `permission denied`,
  collecting every unusable root in manifest order.
- **Docs**: `docs/reference/manifest.md` gains the `[route.static]` row and
  `## Static` section, the `app` row becomes `no*` with the all-static rule, and
  the boot list gains the static refusals. `docs_fences.rs` is now seven files
  and pins the static page's shape with a drift canary.
- No dependency, `Cargo.toml`/`Cargo.lock`, Makefile or e2e change. Every existing
  suite is untouched and green.

## Evidence

### `CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_static_manifest --locked`

```console
test every_bad_immutable_pattern_names_the_rule_it_breaks ... ok
test every_bad_index_names_the_rule_it_breaks ... ok
test every_bad_root_names_the_rule_it_breaks ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

### `CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test docs_fences --locked`

```console
test every_documented_manifest_parses_and_validates ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### `CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked` (every existing suite, untouched)

```console
test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # src unit
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # critical_manifest_011
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # critical_manifest_edit
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # critical_static_manifest (new)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out     # docs_fences
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # edit_branches
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # grants
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # mutation_gaps
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # parse
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # routing
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out     # uid
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # validate
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out     # doctests
```

### `cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings`

```console
    Checking blastit-manifest v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-manifest)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.17s
```

### `RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-manifest --no-deps --locked`

```console
 Documenting blastit-manifest v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-manifest)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.51s
   Generated /home/remy/offworld-nexus/blastit/target/doc/blastit_manifest/index.html
```

### `rustfmt --edition 2024 --check` on every touched `.rs` file, then `cargo fmt --all -- --check`

```console
$ rustfmt --edition 2024 --check crates/blastit-manifest/src/{model,parse,validate,error,lib}.rs \
    crates/blastit-manifest/tests/{critical_static_manifest,docs_fences}.rs
EXIT=0
$ cargo fmt --all -- --check && echo "CARGO FMT CLEAN"
CARGO FMT CLEAN
```

### `make rs-lint`

```console
    Checking blastit-manifest v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-manifest)
    Checking blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Checking blastit-env v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-env)
    Checking blastit v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit)
    Checking blastit-py v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.28s
```

### `make rs-test`

```console
$ make rs-test > /tmp/opencode/mb23-rs-test.log 2>&1; echo "EXIT=$?"
EXIT=0
$ grep -c "test result: ok" /tmp/opencode/mb23-rs-test.log
79
$ grep -E "FAILED|^error" /tmp/opencode/mb23-rs-test.log | head -5
(no output)
$ tail -3 /tmp/opencode/mb23-rs-test.log
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### `make docs-check`

```console
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.29s
```

### `make docs-unit`

```console
...............................................................                       [100%]
63 passed, 57 subtests passed in 0.37s
```

### `make critical-guard`

```console
$ make critical-guard BASE=develop NODE=MB.2.3
critical-guard: develop..working tree, 242 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_check_engine.rs            [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_config_and_presets.rs      [VIOLATION: not covered by any node path]
  M  python/tests/critical/test_check_contract.py             [VIOLATION: not covered by any node path]
critical-guard: FAILED: 4 protected critical test(s) changed or deleted without a covering path in node(s) MB.2.3.
```

The literal acceptance command fails on **content this branch already carries**:
`release/0.2.0` (`31c9739`) is 242 files ahead of `develop`, and those four
protected files were changed by earlier `MB.1.*` nodes, not here. `git status`
lists no modification to any of them in this worktree; the only critical file
this node touches is the new one. At the branch base the guard passes:

```console
$ make critical-guard BASE=31c9739 NODE=MB.2.3
critical-guard: 31c9739..working tree, 17 changed file(s)
node MB.2.3: paths [... 8 paths ...]
  no change to a protected critical test
```

## Files

| File | Change |
|---|---|
| `crates/blastit-manifest/src/model.rs` | `StaticPolicy`, `Route::static_policy`/`with_static`, `Manifest::{is_all_static, static_roots, check_static_roots}`, `root_reason` (+135/−1) |
| `crates/blastit-manifest/src/parse.rs` | `static` reserved route key, `[route.static]` reader with the three defaults, misplaced-key walk arm (+65/−4) |
| `crates/blastit-manifest/src/validate.rs` | `check_static` and the four rule functions, the dead-declaration rule, the fixed rule phrases (+204/−1) |
| `crates/blastit-manifest/src/error.rs` | the five variants and their messages (+71) |
| `crates/blastit-manifest/src/lib.rs` | re-export `StaticPolicy`, crate-doc bullets (+8/−3) |
| `crates/blastit-manifest/tests/critical_static_manifest.rs` | **new protected suite**, 15 tests, every accepted form paired with its denied twin (742 lines) |
| `crates/blastit-manifest/tests/docs_fences.rs` | seven files, the page's shape pinned, drift canary (+79/−6) |
| `docs/reference/manifest.md` | `[route.static]` row + `## Static`, `app` `no*` + all-static footnote, boot list (+53/−4) |

Line budget: **615 production+docs lines, 1357 including the new suite** — see
Deviation 9.

## Deviations and interpretations (named, not silent)

1. **`is_all_static` is stricter than the design's one-line comment.** §4.1
   writes `// app().is_none() && !routes().is_empty()`, while `R13` and §4.7
   step 3 say the app is optional **iff every route is static**. Implemented the
   `R13` rule (`no app && routes non-empty && every route has a policy`): the
   loose form would let a mixed app-less manifest boot with a forwarding route
   that has nothing to forward to. `MB.2.6` consumes the predicate unchanged.
2. **`spa = false` with a missing `index` is accepted, not denied.** The dispatch
   asked for that as a denied twin, but the design checks roots only (§4.1) and
   makes a missing fallback a runtime `404` (§4.3 step 8). Pinned as accepted in
   `a_listable_root_is_accepted_while_its_index_need_not_exist_yet`; the denied
   twins in that area are the `StaticRootUnusable` rows.
3. **`immutable` without a wildcard is accepted.** The dispatch asked for a
   "hashed pattern" denial; the design's own fixture D uses `["/index.html"]`,
   a literal pattern, and `*` is the only documented metacharacter. Denials
   stay on the documented rules (leading `/`, `..`, control byte, 32/256).
4. **A mixed static+app manifest without `app` is not refused by `validate`.**
   `BLASTIT_APP` may supply the callable, so the manifest cannot decide; the
   boot refusal is `MB.2.6`'s. This node pins the two manifest-level facts the
   boot uses: `is_all_static() == false` for the mixed manifest, and the static
   route's declarations are `StaticDeadDeclaration` with
   `the manifest declares no app`.
5. **`[route.<proxy>]` is rendered with the proxy name** (`[route.db]`): the
   design's angle-bracket form is a placeholder, and naming the real table is
   the actionable message.
6. **A missing `root` uses the existing `MissingKey`** (`route[0].static.root:
   missing required key`), not a sixth variant: the design's five variants all
   carry a written value.
7. **An empty `root` is `must be a relative path`** (the closest fixed phrase);
   an empty `index` is `must be one or more segments`. Both are the design's
   fixed phrases, no new wording.
8. **`root_reason`'s fallback for any other `io::ErrorKind` is
   `cannot be read`** (symlink loop, broken mount): still a fixed phrase, still
   no `strerror`; the three documented kinds are exact. Unreachable in the
   tested matrix.
9. **The ≤ 600 changed-line budget is exceeded** (615 production+docs, 1357 with
   tests). The overage is rustdoc on five public variants plus the protected
   suite's table verbosity (the design's §7.2 needs 15 test groups and 5 exact
   messages); no second concern was bundled and no key, flag or error outside
   `R1`–`R5`/`R13` was added.

## Follow-ups

1. `crates/blastit-manifest/tests/parse.rs:422` pins `ROUTE_KEY_EXPECTED`, whose
   enumeration (`name`, `path`, `methods`, `groups`, `egress`, `prefork`,
   `ingress`) now omits `static`. Out of this node's paths; needs a one-line
   test + parse.rs string update so the "unknown route key" message lists all
   eight.
2. `MB.2.2` should reconcile design §4.1's `is_all_static` comment with `R13`
   (Deviation 1) so `MB.2.6` and the design agree in words.
3. `[route.static] groups = […]` reports `groups after a [table] is not
   top-level` (via the shared misplaced-key walk) rather than an unknown static
   key. Consistent with `[route.ingress]`, but the second message would be more
   direct if the walk ever learns table-specific hints.
4. Two host gotchas worth `docs/knowledge/LESSONS.md`, which is outside this
   node's paths: TOML forbids a literal NUL in a basic string, so a
   control-byte fixture needs `\u0000` (a Rust `\u{0}` in the source is invalid
   TOML); and `std::fs::metadata("/proc/self").uid()` is a `geteuid()` oracle
   without a `libc` dependency, for the "permission denied" row.

## Post-work addition (`paths` extended: `crates/blastit-manifest/tests/parse.rs`)

- `ROUTE_KEY_EXPECTED` gained `static` as its last route key, so a mistyped route key is told the reserved block exists; `tests/parse.rs::an_unknown_route_key_that_is_not_a_table_is_an_error` pins the exact expected string and asserts the rendered message is **one line** and mentions `static` (Follow-up 1 closed).
- `CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked` → every suite `ok` (`parse` 33 passed, `critical_static_manifest` 15, `docs_fences` 5), 0 failed; `cargo fmt --all -- --check` → `FMT_CLEAN`; `cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings` → `Finished`.
