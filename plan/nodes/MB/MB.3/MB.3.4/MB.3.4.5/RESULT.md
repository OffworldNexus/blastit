# Summary

Closed accepted MINOR (b) from `docs/testing/AUDIT-MB.3.4.md`: the §4.6.2
8 192-byte value rule now lives in the public `lockdown::resolve`, so no public
function of `blastit-fastcgi` hands out an over-long `CONTENT_TYPE`.

One file changed: `crates/blastit-fastcgi/src/lockdown.rs`.

- **`resolve`** (`b"CONTENT_TYPE"` arm) returns `Ok(None)` when
  `facts.content_type` is longer than `MAX_PARAM_VALUE_BYTES`, and
  `Ok(Some(fact_value(..)))` otherwise. This is the single source of truth for
  the cap.
- **`build`** lost its duplicate pre-check. It now records the drop when
  `resolve` answers `None` for a *present* `CONTENT_TYPE`:
  `None if name == b"CONTENT_TYPE" && facts.content_type.is_some()` → push
  `b"CONTENT_TYPE"` once. An absent header stays `Ok(None)` with no drop, and
  `CONTENT_LENGTH`'s `None` is untouched.
- **Docs**: `resolve`'s rustdoc states the cap and why `REQUEST_URI` /
  `QUERY_STRING` stay verbatim (target bytes, not header values);
  `MAX_PARAM_VALUE_BYTES`'s doc now names `CONTENT_TYPE` as well as `HTTP_*`
  (the crate has `#![deny(missing_docs)]`).
- **One new unit test** in `lockdown::tests`:
  `resolve_applies_the_value_cap_to_content_type` asserts the exact boundary —
  8 192 bytes → `resolve` `Ok(Some(len 8 192))` and `build` emits it with no
  drop; 8 193 bytes → `resolve` `Ok(None)`, `build` emits no `CONTENT_TYPE`
  and `dropped() == [b"CONTENT_TYPE"]`; `content_type: None` → `Ok(None)` and
  no drop.

**Deviation from the node's acceptance text (stated, not chased).** NODE.md
says "42 unittests / 25 critical_fastcgi_lockdown" and "41 + the new resolve
test"; that predates `MB.3.4.3` (which hardened the protected lockdown suite)
and `MB.3.4.4` (which added `critical_fastcgi_codec`). Measured baseline at
base `612ecad` is 43 unit + 12 `critical_fastcgi_codec` + 30
`critical_fastcgi_lockdown` + 1 doc-test, so this node's +1 unit test lands on
**44 / 12 / 30 / 1** exactly as the dispatch predicted.

# Evidence

## 1. Whole crate (`cargo test -p blastit-fastcgi --locked`)

```console
$ cargo test -p blastit-fastcgi --locked
     Running unittests src/lib.rs (...)
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fastcgi_codec.rs (...)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fastcgi_lockdown.rs (...)
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests blastit_fastcgi
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The new test is the only unit-test delta (43 → 44); both protected suites are
unchanged in count and all 30 protected lockdown tests pass.

## 2. Protected lockdown suite, and the protected file is untouched

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

```console
$ git diff 612ecad -- crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs | wc -c
0
```

`over_long_content_type_is_dropped_not_refused` is green *unmodified*: 8 192
emitted, 8 193 dropped and recorded once, `u64::MAX` `CONTENT_LENGTH` emitted.

## 3. Lint, docs

```console
$ make rs-lint
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.20s
$ exit=0
```

```console
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-fastcgi --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.52s
    Generated .../target/doc/blastit_fastcgi/index.html
$ cargo fmt --all -- --check
FMT_OK
```

## 4. Mutation campaign (`make mutants-rust PKG=blastit-fastcgi`)

Scratch: `MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-5-mut`.

```console
Found 352 mutants to test
MISSED   crates/blastit-fastcgi/src/lockdown.rs:185:9: replace Params::is_empty -> bool with true
MISSED   crates/blastit-fastcgi/src/lockdown.rs:589:32: replace | with ^ in hex_pair
MISSED   crates/blastit-fastcgi/src/param.rs:60:37: replace | with ^ in encode_len
352 mutants tested in 84s: 3 missed, 331 caught, 18 unviable
```

18 unviable as expected. The only survivors are the three pre-existing
tolerated equivalences — no new survivor in the cap:

- `len > MAX_PARAM_VALUE_BYTES` → `>=` makes 8 192 answer `None`, caught;
- `>` → `<` / `==` flips both boundary sides, caught;
- `facts.content_type.is_some()` → `is_none()` fails the 8 193-drop assertion
  and the absent-header no-drop assertion, caught.

The same unit test also kills the `Ok(Some(fact_value(value)?))` and `None`
arm changes.

## 5. Full fast gate

```console
$ make check
...
57 fences: 30 executed, 27 accounted, 0 failed · 20.0s
$ uv run --no-project tools/plan.py validate
plan OK
$ exit=0
```

## 6. Security self-audit

- No `unwrap`/`expect`/`panic!` on the production path; the only `expect`s are
  inside the new `#[cfg(test)]` test.
- The over-cap path now returns `Ok(None)` *before* `fact_value`'s `to_vec`,
  so it allocates less than before; no new allocation or loop proportional to
  untrusted bytes.
- The drop record is the name `b"CONTENT_TYPE"` only; `Params`' `Debug` still
  prints names, never values, so no value is logged.
- The cap is still "dropped, never refused", and `CONTENT_TYPE` is still not
  emitted as `HTTP_*`.

# Files

- `crates/blastit-fastcgi/src/lockdown.rs` — `resolve` cap + rustdoc, `build`
  drop recording, `MAX_PARAM_VALUE_BYTES` doc, one new unit test. No other
  file touched.

# Follow-ups

- None. The invariant is intentionally scoped to header-derived values:
  `REQUEST_URI`/`QUERY_STRING` are target bytes and §4.6.2 keeps them verbatim,
  so they are not subject to `MAX_PARAM_VALUE_BYTES`.
