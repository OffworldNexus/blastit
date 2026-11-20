# MB.56.2.3 — Collapse `blastit-manifest`'s §7 suite (15k → 1.5k LOC)

## Summary

`crates/blastit-manifest/tests/` holds **five files, 1 500 LOC** (each under 500),
all keyed to `TESTING_PLAN.md` §7 "`Blastfile` language & surgical editing":

| file | LOC | covers |
|---|---|---|
| `blastfile_language.rs` | 360 | grammar surface, positioned diagnostics, duplicate declarations, boot warnings |
| `blastfile_routing.rs` | 368 | route precedence, path patterns, methods, trait composition, grant rows |
| `blastfile_endpoints.rs` | 106 | `body` codecs, PHP entrypoint rules |
| `blastfile_format.rs` | 351 | comment-preserving fixed point + every published/fragment `Blastfile` fence |
| `blastfile_edit.rs` | 315 | surgical grant insertion, refusals, catch-all placement |

Deleted **33 fragmented suites + `tests/common/mod.rs` + `tests/fixtures/` (15 files)**
(14 499 test LOC) and the **13 inline `#[cfg(test)]` modules in `src/**`** (4 493 LOC).
`git diff --numstat -- crates/blastit-manifest/src` reports `0` insertions everywhere:
production code is unchanged, only trailing test modules were removed.

## Fixes from review

* **Docs walker**: classifies a fence by its info string — language `blastfile`, or a
  `title` **starting with** `Blastfile`. A complete file (`title="Blastfile"`) must
  parse, lower to ≥1 view/mount, and be `fmt`-fixed; a fragment (e.g.
  `docs/deploy/digitalocean-paas.md`'s `title="Blastfile: the store block"`) must
  parse and be `fmt`-fixed, with no view/mount requirement. Discovery is dynamic over
  `README.md` + `docs/**` minus `docs/{design,decisions,research,knowledge,testing}/`
  and `docs/VISION.md`; no per-page count table and the skip-list is empty. The walk
  asserts ≥10 published pages, ≥10 complete files and ≥1 checked fragment, so no
  discovered fence is silently skipped.
* **Public examples canonicalised** in `docs/reference/blastfile.md` (PHP, stores,
  view header, cart, `use session`) — verified against `blastit fmt` itself.
* **Secret oracle**: `expect_err` plus an empty-list guard, then every rendered error
  is scanned for `hunter2` (it can no longer pass on `Ok`).
* **Warnings**: `EgressRoute` and `UngrantedProxy` are asserted through
  `parse_blastfile`; duplicate `view`/`trait`/`store` and `DuplicateMount` have exact
  `line:col`/sentence cases.
* **Cohesion**: split into the five files above (`language`/`routing`/`endpoints`/
  `format`/`edit`), each < 500 LOC, 1 500 total.

## Coverage map (old behaviour → new owner test)

| New test | Keeps |
|---|---|
| `the_documented_surface_parses_and_lowers` | `critical_blastfile_parity` rows (app/entrypoint, stores, mount, grants, body, multi-endpoint, regex path, `GET`→`HEAD`), `mutation_gaps` accessors |
| `the_grammar_refusal_matrix_is_positioned` | `critical_blastfile_parser::the_refusal_matrix`, `blastfile_errors`, duplicate section refusals |
| `a_refusal_never_echoes_a_pasted_secret` | `critical_blastfile_security::a_value_is_never_echoed` |
| `a_duplicate_mount_and_the_boot_warnings_are_reported` | `blastfile_errors` duplicate mount, `validate` `EgressRoute`/`UngrantedProxy` |
| `body_codecs_lower_and_raw_warns_at_boot`, `the_php_entry_rules_…` | `blastfile_body`, `critical_blastfile_body`, `blastfile_entrypoint`, `critical_blastfile_entrypoint`, `critical_php_manifest` |
| `route_precedence_…`, `the_pattern_table_…`, `the_pattern_refusals_…`, `methods_…` | `routing.rs`, `blastfile_patterns`, `critical_blastfile_patterns` |
| `trait_composition_…`, `grants_lower_…` | `blastfile_traits`, `critical_blastfile_traits`, `grants.rs`, `blastfile_grants` |
| `formatting_…` (three tests) | `blastfile_fmt` P1–P3 + cap, `critical_blastfile_print` |
| `every_documented_blastfile_fence_…`, `a_broken_published_example_…` | `docs_fences` (now documents **and** fragments, dynamic discovery) |
| `an_edit_…`, `a_grant_lands_…` (four tests) | `critical_manifest_edit`, `critical_manifest_edit_executor`, `edit_branches`, `grant_fragment` |

## Deliberately dropped (and why)

* **Private CST/lexer/token invariants** (inline `lexer`/`parser`/`syntax`/`tokens`/
  `print` modules, `critical_blastfile_lexer`, the parser depth test): `Cst`/`Token`
  are `pub(crate)`; the fixed point and the exact-position matrix prove the same
  properties through the public API.
* **`blastfile::read` path shapes** (FIFO/device/16 MiB/mode 000): a boot/CLI concern;
  the size cap is still exercised by `parse` and by `fmt`'s growth refusal.
* **`write_atomic` durability** (temp names, symlinks, stale temps, `EACCES`): the
  filesystem writer is stage-0/CLI; `MB.56.5` prunes it.
* **`UidLayout` arithmetic** (`uid.rs`): §6 containment, not §7.
* **`grant_fragment`/`table_grants` paste rendering**: owned by `blastit-sqlpolicy`.
* **Retired TOML rows** (`critical_manifest_011`): the format is gone (`ADR-0012`).
* **`QuerySite`/`sites.rs` positions** (`critical_query_sites`, `warning_sites`): the
  editor-diagnostic surface belongs to §7's language-server bullet, not the grammar.
  `src/blastfile/sites.rs` is reached only through the LSP path, so its 0 % line
  coverage in this crate's tests is expected and the module **must be kept**.
* **Inline `#[cfg(test)]` modules in `src/**`**: private helpers whose public effects
  are asserted above.

## Evidence

```console
$ cargo test -p blastit-manifest --locked
  blastfile_edit: 4 passed;  blastfile_endpoints: 2 passed;  blastfile_format: 5 passed
  blastfile_language: 4 passed;  blastfile_routing: 6 passed;  doc-tests: 7 passed
$ wc -l crates/blastit-manifest/tests/*.rs
  1500 total   (5 files, each < 500)
```
