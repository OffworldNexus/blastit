---
id: MB.21.3.8
title: "Lower the Blastfile CST into Manifest against the expected model, with the parity oracle, the nine suites and the fixtures"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.3]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/tests/blastfile_grants.rs"
  - "crates/blastit-manifest/tests/blastfile_traits.rs"
  - "crates/blastit-manifest/tests/blastfile_body.rs"
  - "crates/blastit-manifest/tests/blastfile_entrypoint.rs"
  - "crates/blastit-manifest/tests/blastfile_patterns.rs"
  - "crates/blastit-manifest/tests/blastfile_errors.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_parity.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_traits.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_body.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_entrypoint.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_security.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_parser.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_patterns.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_lexer.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_errors.rs"
  - "crates/blastit-manifest/tests/common/mod.rs"
  - "crates/blastit-manifest/tests/fixtures/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T02:16"
updated: "2026-11-16T03:35"
---

# Goal

Lower the CST into `Manifest` and prove it with the model-equality oracle.

- `blastfile/lower.rs`: CST → `Manifest` (+ the validation hooks), reading the arena through typed
  accessors — no second AST (§4.2, §6.4, §7).
- The two public entry points of §4.4: `Manifest::from_blastfile_str` (parse + lower, no validate) and
  `Manifest::parse_blastfile` (parse + lower + validate, returning warnings).
- The capability map of §7 row by row, with grants (§13) in the shape `PgGrant`/`TableGrants` already
  has, the two `raw` forms (§10), traits and `use` (§11), entrypoints and `prefork`/`egress any`
  (§12).
- The **oracle**: for each fixture, `Manifest::equivalent` against the expected `Manifest` built in the
  test, with every row the design drops asserted **absent**.
- The two pattern suites `MB.21.3.3` could not write, because they need this leaf's public entry point:
  `tests/blastfile_patterns.rs` (the §8.1 table and every §8.3 refusal through `from_blastfile_str`) and
  the protected `tests/critical_blastfile_patterns.rs` (the traversal corpus, a 257-byte pattern, an
  unbalanced `)`, and `HEAD` present in `methods` for a `GET` view).

This leaf is **design `MB.21.3.2b`** (§21). The plan tool has no letter-suffixed ids, so the parser half
is `MB.21.3.2` and this half is `MB.21.3.8`.

# Context

- Design: §4.4 (the surface), §7.1–§7.3 (the capability map, `route_lines`, `check_uids`), §10–§13,
  §20 (the oracle and the R→test table), §21 (this leaf's paths and acceptance).
- **The oracle is not a comparison against the retired reader.** There is no `from_toml_str` call, no
  TOML twin and no cross-format test anywhere in this leaf: the expected `Manifest` is constructed
  field by field in the test, and the design's dropped rows (`version` header, `workers`, per-route
  `static`/`php`, the global `groups` attach) are asserted **absent** rather than merely uncompared.
  `MB.21.3.2`'s in-module refusal matrix is repeated here through the public entry points in the
  protected suite, so the matrix is assertable from outside the crate.
- Fixtures live under `crates/blastit-manifest/tests/fixtures/` and are the corpus `MB.21.3.4` formats,
  `MB.21.3.6` counts and `MB.21.5.1` documents: one per capability-map row plus the denial-side ones.
  Their shape is a decision this leaf records in `RESULT.md` (one directory per row, `*.blastfile`
  plus the expected model in the test), because three later leaves read them.
- Reuse `validate.rs`; do not re-implement it (§7.3, `MB.3.21`'s lesson). `Route::ingress`, `egress`,
  `prefork` and the grant types keep their names; `query` keeps the existing `normalize` spelling, so
  `check`'s `dead_grant`/`missing_grant` read the same strings.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_parity
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ make check
```

1. `critical_blastfile_parity.rs` enumerates one fixture per capability-map row, names the row in the
   test name, and compares with `Manifest::equivalent` against an expected `Manifest` built **in the
   test** — a reviewer can read both sides without leaving the file.
2. The dropped rows are proven absent: a test asserts that a `view` cannot carry a static or PHP
   declaration, that `workers` is refused with `line:col`, and that no lowering path produces a
   non-empty global `groups` attach.
3. Every refusal the design pins (§6.7, §10, §11.3, §13.1) is asserted through the public entry point
   with message **and** `line:col`; none defaults to a permissive value.
4. `critical_blastfile_{traits,body,entrypoint,security}.rs` and `blastfile_{traits,grants,body,entrypoint}.rs`
   pass, and `parse_blastfile` surfaces the `validate` warnings unchanged (`UnattachedGroup`,
   `UngrantedProxy`, `UnrestrictedRoute`, `RawIngress`, `EgressRoute`, `EntrypointNotReached`).
5. A hostile document (2 block levels is the grammar's maximum; `{{{{…` and a 256 KiB file of views)
   fails or finishes cleanly — never a stack overflow, never an unbounded loop.
6. `make check` exit 0; the retired reader is untouched and its suites are still green.
7. The two pattern suites pass: `blastfile_patterns.rs` proves the §8.1 table through
   `from_blastfile_str` (`/a.b` ≠ `/axb`, `{id:int}` rejects `12a`, `{id:uuid}` rejects a near-miss,
   `/assets/*` matches a nested file, a bare `/x` does not match `/xy`) and `critical_blastfile_patterns.rs`
   proves the refusal matrix with message and `line:col`, a `GET` view's `methods` carrying `HEAD`, and
   the protected caps (§4.5 `MAX_PATTERN_BYTES`, the traversal corpus, an unbalanced `)`).

# Notes

- `crates/blastit-manifest/tests/critical_blastfile_parser.rs` lands here, not in `MB.21.3.2`: an
  integration suite may use only the public entry points (§4.1), so the parser's refusal matrix is
  in-module there and public here.
- `crates/blastit-manifest/tests/common/mod.rs` gains the fixture loader the nine suites share; it is in
  `paths` for that reason.
- `src/model.rs` is in `paths` for `Route::with_sandbox` plumbing only — the model itself was finished
  by `MB.21.3.0`.
