# Result MB.21.3.7

## Summary

Landed the second half of the model parity: `StaticPolicy.spa: bool` is gone and
`StaticPolicy.fallback: Option<String>` replaces it (§9.3), with the retired reader lowering
`spa = true` to `fallback = Some(index)` so `blastit.toml` parses and serves exactly as before. The
three readers of the removed flag derive it from the new field (`validate.rs`'s two "answers every
miss itself" refusals keep their exact sentences, `route.rs`'s `static_forward`, `static_asset.rs`'s
internal flag). §9.1's mount rules landed in `validate.rs` (prefix shape, `fallback` dotfile,
`DuplicateMount`, a new `Location::Mount`), §10.3's `check_routes` now keys on **sandbox** so two
endpoints of one view are legal, `TooManyRoutes` names "the route carrying the highest sandbox", and
`tests/parse.rs` pins the reader's bridge. `make check` exits 0; `critical_guard` is clean.

**Two deviations from the node, both disclosed in `## Deviations` below** — read that section first:
(a) two `StaticPolicy` literals live in `src/routing.rs`/`src/uid.rs`, which `paths` does not cover;
(b) the node's item 6 ("two `[route.static]` blocks → two mounts") and its item 4 (`DuplicateMount`)
cannot both hold, because a protected test pins that a three-static-route TOML validates.

## Deviations (read before reviewing)

### D1. Two `StaticPolicy` literals outside `paths` — `crates/blastit-manifest/src/routing.rs:274`, `src/uid.rs:163`

The node's `# Context` says "every `StaticPolicy { … }` literal in the workspace is in a file of this
leaf's list". That measurement missed the two **in-crate unit-test** literals `MB.21.3.0` itself
added when it wrote `routing.rs`'s `mounted()` helper and `uid.rs`'s `a_mount_owns_no_uid`. A struct
literal must name every field, so removing `spa` breaks them, and no in-path edit can fix that. The
change is one line each and mechanical:

* `crates/blastit-manifest/src/routing.rs:274` — `spa: false,` → `fallback: None,`
* `crates/blastit-manifest/src/uid.rs:163` — `spa: false,` → `fallback: None,`

No assertion, test name or behaviour changed. `critical_guard` does not guard these files (they are
not `tests/critical_*`), and `make check` is green with them. **The epic must widen this node's
`paths`** (or accept the two lines) — see `# Follow-ups` 1.

### D2. `mounts_of` keeps one mount per prefix (first block wins), not one per block

The node asks for both "two `[route.static]` blocks → two mounts" (item 6) and "§9.1 refuses a
duplicated prefix (`DuplicateMount`)" (item 4). With the reader emitting one mount per block, a TOML
file with two or more static routes gets two or more mounts at `"/"`, and `DuplicateMount` refuses
it — which breaks
the **protected** test `critical_static_manifest.rs::every_unusable_root_is_named_and_none_stops_the_list`
(it parses three static routes and expects them to validate). Measured, not guessed:

```console
$ # with one mount per block and §9.1's duplicate rule in place:
thread 'every_unusable_root_is_named_and_none_stops_the_list' panicked at
  crates/blastit-manifest/tests/critical_static_manifest.rs:635:10:
the manifest validates: ManifestErrors([DuplicateMount { prefix: "/", first: 0, second: 1 },
                                       DuplicateMount { prefix: "/", first: 0, second: 2 }])
```

The node's own tie-breakers ("behaviour does not change", "no protected suite loses a claim", "never
invent a rule") leave one shape: the reader's mount list holds **one mount per prefix**, the first
`[route.static]` block winning — exactly the bridge `php_of` already makes for the one front
controller, and the only shape in which §9.1's uniqueness invariant is true of every `Manifest` the
crate can build. Nothing on the runtime path reads `mounts` yet (grep: no consumer outside
`blastit-manifest`), so `blastit.toml` still parses and serves exactly as today; every route keeps
its own `[route.static]` block untouched. `tests/parse.rs` pins the bridge as it now is, and
`parse.rs`'s builder doc comment says so. If the epic prefers two mounts, `DuplicateMount` must move
to `MB.21.3.8`'s lowering (where positions exist anyway, §18) and this leaf loses acceptance 4.

## Evidence

### 1. `make rs-fmt` (exit 0) and `make rs-lint` (exit 0, `-D warnings`)

```console
$ make rs-fmt
cargo fmt --all -- --check
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
    Checking blastit-env v0.2.0 (…/crates/blastit-env)
    Checking blastit v0.2.0 (…/crates/blastit)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.40s
$ make rs-doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.20s
   Generated …/target/doc/blastit/index.html and 13 other files
```

### 2. `cargo test -p blastit-manifest --locked` (exit 0)

```console
test result: ok. 58 passed; 0 failed   # lib (53 before: +5 new unit tests)
test result: ok. 17 passed; 0 failed   # critical_manifest_011
test result: ok. 23 passed; 0 failed   # critical_manifest_edit
test result: ok. 5 passed; 0 failed    # critical_manifest_edit_executor
test result: ok. 9 passed; 0 failed    # critical_php_manifest
test result: ok. 15 passed; 0 failed   # critical_static_manifest
test result: ok. 6 passed; 0 failed    # docs_fences
test result: ok. 19 passed; 0 failed   # edit_branches
test result: ok. 13 passed; 0 failed   # grants
test result: ok. 20 passed; 0 failed   # mutation_gaps
test result: ok. 39 passed; 0 failed   # parse (38 before: +1 bridge test)
test result: ok. 11 passed; 0 failed   # routing
test result: ok. 8 passed; 0 failed    # uid
test result: ok. 29 passed; 0 failed   # validate
test result: ok. 6 passed; 0 failed    # doctests
```

### 3. `cargo test -p blastit-scheduler --locked` (exit 0) and `cargo test -p blastit --locked` (exit 0)

```console
$ cargo test -p blastit-scheduler --locked
17 × "test result: ok. …; 0 failed"   # lib 51, critical_rewrite 32, critical_static_assets 14,
                                      # critical_static_entry 6, critical_static_resolve 18, …
$ cargo test -p blastit --locked
15 × "test result: ok. …; 0 failed"   # lib 210 (1 ignored), critical_check_engine 19,
                                      # critical_static_cli 11, no_python_build 3, smoke 31, …
```

### 4. Acceptance item 1 — the field changed and the retired key still lowers

```console
$ grep -n 'spa' crates/blastit-manifest/src/model.rs
$ echo $?
1
```

`StaticPolicy` is now `{ root, index, fallback: Option<String>, immutable }`; no `spa` field, and the
three doc comments that merely *contained* the substring (`spawns`, `travels`, `dispatcher`) were
reworded so the acceptance grep prints nothing. The reader still accepts the key:

```console
$ grep -rn 'spa = true' crates/blastit-manifest/tests/critical_static_manifest.rs \
      crates/blastit-manifest/tests/critical_php_manifest.rs
critical_static_manifest.rs:32:spa = true
critical_static_manifest.rs:146:            // The page's `spa = true` lowered to the file it answers a miss
critical_static_manifest.rs:222:        parse_errors(&static_manifest("spa = true\n")),
critical_static_manifest.rs:451:    let spa_why = "spa = true never reaches the app";
critical_static_manifest.rs:454:        "root = 'public'\nspa = true\n\n[route.ingress]\nbody = \"json\"\n",
critical_static_manifest.rs:459:        r#"route "site" (index 0): spa = true never reaches the app; [route.ingress] can never run"#
critical_static_manifest.rs:465…484  (three more `spa = true` fixtures)
critical_php_manifest.rs:291…363     (the PHP dead-declaration pair, `key: "spa = true"`)
```

Every one of those fixtures keeps its assertion: `spa = true` becomes `fallback = Some(index)`, so
`the_pages_manifest_parses_with_the_defaults_it_omits`, the `spa = true never reaches the app`
refusal and the PHP `spa = true never reaches the front controller` refusal all still pass unchanged
in wording. `critical_php_manifest.rs` needed **no edit at all** (it has no `StaticPolicy` literal).

### 5. Acceptance item 3 — the protected suites changed only where the field name changed

```console
$ uv run --no-project tools/critical_guard.py --base ef783db0c5c094a9f6aff5e2042e7955c40cadb5 --node MB.21.3.7
critical-guard: ef783db0c5c094a9f6aff5e2042e7955c40cadb5..working tree, 21 changed file(s)
node MB.21.3.7: paths [...]
  M  crates/blastit-manifest/tests/critical_manifest_edit_executor.rs  [authorized by …]
  M  crates/blastit-manifest/tests/critical_static_manifest.rs  [authorized by …]
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [authorized by …]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.21.3.7
```

Every edit, old → new (no assertion removed or weakened):

| File | Old | New |
|---|---|---|
| `critical_static_manifest.rs:146` | `spa: true,` | `fallback: Some("index.html".to_string()),` (+ 2-line comment naming §9.3) |
| `critical_static_manifest.rs:157` | `spa: false,` | `fallback: None,` |
| `critical_static_manifest.rs:509` | `assert!(!route.static_policy().expect("policy").spa);` | `assert!(route.static_policy().expect("policy").fallback.is_none());` |
| `critical_manifest_edit_executor.rs:192` | `spa: false,` | `fallback: None,` |
| `critical_rewrite.rs:622` | `spa: false,` | `fallback: None,` |
| `critical_rewrite.rs:653` (helper `policy(spa: bool)`) | `spa,` | `fallback: spa.then(\|\| "index.html".to_string()),` (+ 2-line comment) |

The helper keeps its `spa: bool` parameter and every call site (`policy(true)`/`policy(false)`), so
each test's claim reads exactly as before; only the literal body moved. `docs_fences.rs` (not
protected) changed one literal the same way.

### 6. Acceptance item 4 — §9.1, both directions

```console
$ cargo test -p blastit-manifest --locked --lib
test validate::tests::a_mount_prefix_is_a_url_prefix ... ok
test validate::tests::two_mounts_with_one_prefix_are_refused ... ok
test validate::tests::a_dotfile_fallback_is_refused_while_a_dot_directory_root_is_legal ... ok
test validate::tests::a_bad_index_beside_the_retired_flag_is_reported_once ... ok
```

* **Prefix shape:** `/`, `/assets`, `/assets/v2`, `/.well-known`, `/a-b_c` are accepted and validate;
  `assets`/`""` → `must start with /`, `/assets/` → `must not end with /`, `//assets` and
  `/assets//v2` → `must not contain an empty segment`, `/assets/../etc` → `must not contain ..`,
  `/assets\v2` → `must not contain \`, `/assets\0` → `must not contain a control byte`,
  `/assets?x=1` → `must not contain ?`, `/assets#top` → `must not contain #`, `/assets%2e` →
  `must not contain %`. Each is asserted as the exact `ManifestError` (rule **and** position
  `Location::Mount { index, prefix }`), not merely "an error", plus the rendered line the operator
  reads — `manifest has 1 error(s)` / `  - mount "/assets/" (index 0): must not end with /`.
* **DuplicateMount:** two mounts at `/assets` (indices 0 and 2) → `DuplicateMount { prefix:
  "/assets", first: 0, second: 2 }`, rendered `  - duplicate static mount "/assets" (mounts 0 and
  2)`; the legal twin (one `/assets`, one `/`) validates.
* **`fallback` dotfile:** `.env`, `shell/.env`, `.output/index.html` are refused with
  `StaticFallbackInvalid { fallback, rule: "must not contain a dotfile segment" }`; `index.html`,
  `app/shell.html`, `./index.html` are accepted. `root ".output"` is legal (a dot-directory the
  author chose) and `index ".env"` is still refused with `StaticIndexInvalid` — `root` keeps today's
  rules, `index` keeps its existing one.
* **One mistake, one line:** `root "." + index ".env" + spa = true` reports the index refusal
  **once** (the mount's `fallback` *is* that index, so the mount check does not repeat it).

### 7. Acceptance item 5 — §10.3 keyed on sandbox

```console
$ cargo test -p blastit-manifest --locked --lib
test validate::tests::two_endpoints_of_one_view_are_not_a_duplicate ... ok
```

Two routes named `cart` in sandbox 0 validate clean (one view, two endpoints); the same name in
sandboxes 0 and 1 is `DuplicateRouteName { name: "cart", first: 0, second: 1 }`. The TOML path is
unchanged by construction — the retired reader numbers one sandbox per route — so
`tests/validate.rs::duplicate_route_names_are_reported` (three routes, `about` at 0 and 2) still
passes **unmodified**, and every `DuplicateRouteName` sentence is byte-identical.

### 8. Acceptance item 6 — the reader's bridge is pinned

```console
$ cargo test -p blastit-manifest --locked --test parse
test a_second_block_of_the_same_kind_never_answers ... ok
test a_static_block_fills_the_mount_list_beside_the_route ... ok   # re-pointed at `fallback`
```

`a_second_block_of_the_same_kind_never_answers` asserts both bridges: two `[route.static]` blocks
leave both routes' own blocks intact (the second still carries `root "dist"`, `fallback
Some("shell.html")`) while `Manifest.mounts()` holds **one** `/` mount carrying the first block; two
`[route.php]` blocks leave both routes' blocks intact while `Manifest.php()` is the first entry.
`parse.rs`'s `parse()` doc comment states the rule ("Both are **first block wins**…"), and
`mounts_of`'s doc comment explains why (see D2).

### 9. Acceptance item 7 — no behavioural change

```console
$ grep -rn "TooManyRoutes\|too many routes" crates/ python/ e2e/ docs/ --include=*.rs --include=*.py --include=*.md
crates/blastit-manifest/src/uid.rs:206          # test constructs the variant
crates/blastit-manifest/src/error.rs:309        # the message
crates/blastit-manifest/src/validate.rs:709     # the check
crates/blastit-manifest/tests/uid.rs:84         # test constructs the variant
crates/blastit-manifest/tests/validate.rs:264,274,319   # tests construct the variant
```

No test or doc asserts the old `TooManyRoutes` sentence, so the re-worded message ("too many routes:
the route carrying the highest sandbox (index N) would use uid U, but uids must stay below M (base
B)") reaches only a boot refusal. No `--json` key moved (`MB.21.4.3` owns that), no exit code
changed, and the only new refusals are for shapes the retired format cannot spell (a non-`/` prefix,
a second mount, a `fallback`), so every `blastit.toml` that parsed before parses and serves as
before — `cargo test -p blastit --locked` (210 unit + 11 `critical_static_cli` + 31 `smoke`) is the
evidence.

### 10. `make check` (exit 0)

```console
$ make check
guard: OK (1 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 21 changed file(s)
cargo fmt --all -- --check
… 131 × "test result: ok", 0 × "test result: FAILED"
No issues found                       # docs-check (zensical --strict)
doc-tone: ok — 28 user-facing page(s), no banned phrase
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed
uv run --no-project tools/plan.py validate
plan OK
make check exit=0
```

Run once, serially, on the final tree (this `RESULT.md` included).

## Files

* `crates/blastit-manifest/src/model.rs` — `StaticPolicy.fallback` replaces `spa`; three doc
  comments reworded so `grep 'spa'` is clean.
* `crates/blastit-manifest/src/parse.rs` — the reader lowers `spa = true` to `fallback =
  Some(index)`; `mounts_of` keeps one mount per prefix; `parse()`/`mounts_of`/`static_policy` doc
  comments state the bridge.
* `crates/blastit-manifest/src/validate.rs` — `check_mounts` (+ `mount_prefix_rule`,
  `fallback_rule`, `is_dotfile_segment`, the six §9.1 rule phrases), `check_routes` keyed by sandbox,
  the two `fallback.is_some()` readers, five new unit tests.
* `crates/blastit-manifest/src/error.rs` — `Location::Mount`, `DuplicateMount`,
  `StaticMountPrefixInvalid`, `StaticFallbackInvalid`, re-worded `TooManyRoutes`.
* `crates/blastit-manifest/src/routing.rs`, `src/uid.rs` — one literal each (**outside `paths`**, D1).
* `crates/blastit-manifest/tests/{critical_static_manifest,critical_manifest_edit_executor,docs_fences,parse}.rs`
  — the field-name moves and the new bridge test.
* `crates/blastit-scheduler/src/route.rs` — `static_forward` from `fallback.is_none()`; test helper
  lowers the flag the way the reader does.
* `crates/blastit-scheduler/src/static_asset.rs` — internal `spa` derived from `fallback.is_some()`.
* `crates/blastit-scheduler/tests/{common/static_fixture,critical_rewrite}.rs` — the field-name moves.

## Follow-ups

1. **`paths` must widen** for `crates/blastit-manifest/src/routing.rs` and `src/uid.rs` (D1): the
   measured `StaticPolicy { … }` sweep missed the two in-crate unit-test literals `MB.21.3.0` added.
   Either add the two paths to this node or keep the two lines as reviewed.
2. **`MB.21.3.8` owns the mount-level `root`/`index` checks and `DuplicateMount`'s positions.** This
   leaf checks the prefix, `fallback` and uniqueness on `Manifest.mounts`, but `root`/`index` are
   still checked on the route's own block (the retired reader's shape). A Blastfile mount has no
   route, so the lowering leaf must check its `root`/`index` too; §18's `line:col` for these §9.1
   refusals also belongs there.
3. **`MB.21.4.2` still owns:** deleting `Route::static_policy`/`with_static`/`php`/`with_php`, the
   `StaticOutcome::SpaLost` → `fallback_missing` rename, §9.5/§9.6's `is_all_static`/`static_roots`
   shape, and `static_asset`'s `missed()` answering the declared `fallback` file rather than
   `self.index` (it keeps a derived flag until then, which is exact for every document this build
   can parse).
4. **Proposed `LESSONS.md` line** (outside `paths`, so recorded here): a sweep for a struct's
   literals must include `#[cfg(test)] mod tests` in *sibling* modules of the same crate — a plan
   that lists "every literal" from `grep` over `tests/` and the named `src/` files will miss
   `src/routing.rs`/`src/uid.rs` and leave the workspace red outside the node's `paths`.
5. **Proposed `LESSONS.md` line:** a validation rule over a *bridged* model list (the reader's
   `Manifest.mounts`) can contradict a protected test that pins the bridge's shape; measure the rule
   against the protected suites before writing the acceptance item, not after.
