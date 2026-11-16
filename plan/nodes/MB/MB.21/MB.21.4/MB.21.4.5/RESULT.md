# Result MB.21.4.5

## Summary

The retired manifest format no longer exists in Rust. `crates/blastit-manifest/src/parse.rs` is deleted
with no replacement, `Manifest::from_toml_str`/`parse_and_validate`/`mod parse` and the two version error
variants are gone, the retired patcher in `src/edit.rs` is gone (`EditError` and `write_atomic` stay), the
ten `ManifestError` variants the reader alone constructed are deleted, the TOML text synthesis in
`stage1::learn_routing_table` is replaced by `RoutingTable::for_learn(&manifest)`, and `toml` left the
crate manifest with its `Cargo.lock` entries. Exactly one reader remains: `blastfile::read` →
`Manifest::from_blastfile_str`.

Six leaves, six commits on `release/0.2.0`:

| Leaf | Commit | What |
|---|---|---|
| `MB.21.4.5.1` | `201c7c2` | the v1 parser suite deleted (39 tests re-homed), the 0.1.1 schema suite translated |
| `MB.21.4.5.2` | `62d4370` | the static-mount / `entrypoint php` grammar in the two protected suites |
| `MB.21.4.5.3` | `736cb20` | the last suites + `docs_fences` counting the retired opening |
| `MB.21.4.5.4` | `f6eb635` | `RoutingTable::for_learn`, the synthesis deleted, env/sqlpolicy examples |
| `MB.21.4.5.5` | `7ba6d23` | the scheduler's 45 fixtures, `Source::Toml` deleted |
| `MB.21.4.5.6` | `f390bd2` | the erasure: reader, patcher, ten variants, `toml` dependency |

## Evidence

Each item of `# Acceptance`, command, result:

1. `grep -rnE 'from_toml_str|toml::|toml_edit' crates/ --include='*.rs'` → no output, `rc=1`.
   `grep -rnE 'blastit[.]toml' crates/ --include='*.rs'` → no output, `rc=1`.
   `cargo tree -p blastit-manifest --locked --edges normal | grep -ci toml` → `0`.
2. `test ! -e crates/blastit-manifest/src/parse.rs` → exit `0`;
   `git grep -nE '^mod parse;' crates/` → no output, `rc=1`;
   `tests/parse.rs` is gone and its 39 `#[test]`s are re-homed row by row in
   `MB.21.4.5.1/RESULT.md` (22 to a named surviving suite — each citation opened and checked — and 15
   recorded as `no successor: <spelling the language does not have>` with a why).
3. `make check` → exit `0`, 4619 lines (`/tmp/opencode/mb2145-batch-makecheck.log`) for the five-leaf
   batch and 4552 lines with 165 `test result: ok` lines at `MB.21.4.5.6`; includes `test-critical`,
   `rs-test`, `rs-lint`, `rs-doc`, `docs-unit`, `docs-check`.
   `make critical-guard BASE=2ef786ce381dce05208d8d4d09a50f8eb4d7589b NODE=MB.21.4.5` →
   `OK: all 9 protected change(s) are authorized by node(s) MB.21.4.5`.
   `make e2e` → exit `0`, 1115 lines (`/tmp/opencode/mb2145-e2e.log`) — run because `crates/blastit`,
   `crates/blastit-manifest` and `Cargo.lock` changed.
4. `git diff --stat 2ef786c..HEAD -- Cargo.toml Cargo.lock` → `Cargo.lock | 55 ------`, `1 file
   changed, 55 deletions(-)`; the workspace `Cargo.toml` was not touched (it never carried `toml` or
   `toml_edit` — confirmed, not assumed) and `crates/blastit-manifest/Cargo.toml` lost exactly one line.
   Only `toml`, `toml_datetime`, `toml_parser`, `toml_writer`, `serde_spanned` and `winnow` left the lock;
   no version bump, no other crate's entry moved.
5. `blastit-env`'s suites build the same wiring from `Blastfile` literals (`cargo test -p blastit-env
   --all-targets` → 50 + 25 passed, `--doc` → 4 passed); `docs_fences` pins a byte-identical per-page
   table to base (`TOTAL [2,0,8]`, README/index/getting-started/frameworks{index,django,flask,php,
   static-and-spa} `[0,0,1]`, `docs/local.md` and `docs/blastfile.md` `[1,0,0]`) with
   `THE_RETIRED_OPENING_IS_GONE` still `false`, and it still executes and validates the two live pages.
   The eight retired pages' fences are counted and named, not parsed — `MB.21.5.1`/`MB.21.5.2` restore
   them as executed fences.

`make check` line counts and the "tree quiescent at commit time" note are recorded in each leaf's
`LOG.md`: one shared attestation per batch, one commit per node.

## Files

Deleted: `crates/blastit-manifest/src/parse.rs`, `crates/blastit-manifest/tests/parse.rs`.
Changed: `crates/blastit-manifest/src/{model,lib,edit,routing,error,validate}.rs`,
`blastfile/mod.rs`, `crates/blastit-manifest/Cargo.toml`, `Cargo.lock`,
`crates/blastit-manifest/tests/{common/mod,validate,mutation_gaps,routing,grants,uid,docs_fences,
static_mounts,critical_manifest_011,critical_static_manifest,critical_php_manifest,blastfile_errors}.rs`,
`crates/blastit/src/{stage1,infer_routes,php_view}.rs`, `crates/blastit/tests/critical_check_engine.rs`,
`crates/blastit-env/src/{env,lib,postgresql}.rs`, `crates/blastit-env/tests/environment.rs`,
`crates/blastit-sqlpolicy/src/{learn,tables}.rs`,
`crates/blastit-scheduler/src/{lib,route,config}.rs`, `crates/blastit-scheduler/tests/**`.

## Follow-ups

### The residual retired-spelling ledger (measured on `f390bd2`)

`MB.21.4.5` erased the reader and every *Rust code path*. Retired **spellings** still appear in strings and
doc comments elsewhere; none is a reader, detector, sniffer or key map. The one class that is not
acceptable — user-facing sentences a developer can be told to follow — is the first table.

**(a) User-facing runtime strings — must be fixed in MB.21.4, not MB.21.5**

| File:line | Today | Reachable from a `Blastfile`? |
|---|---|---|
| `crates/blastit/src/check.rs:457` | `declare [proxy.<name>] with protocol = "postgresql"` | yes — `blastit check` with no `postgres` store |
| `crates/blastit/src/check.rs:492` | `name them in [proxy.{}]` | yes — same path, store declares no input variables |
| `crates/blastit/src/audit_v1.rs:1528` | `set transaction_control = true on [route.{}]` | yes — `plan.emit_hints()` on any `--learn` run (the grammar keyword is `transactions`, so the hint is doubly stale) |
| `crates/blastit-py/src/audit.rs:1553` (+ prefix pass-through `:366`) | JSON `"toml": "[route.<proxy>.tables]…"` | yes — `_blastit.audit()`/`covering()` output |
| `crates/blastit-manifest/src/validate.rs:556` | `dead("[route.ingress]")` | no — needs `route.static_policy()`, which `lower` never sets |
| `crates/blastit-manifest/src/validate.rs:562` | `dead(format!("[route.{proxy}]"))` | no — same gate |
| `crates/blastit/src/stage0.rs:1424` | boot refusal `route "app": [route.php] workers = …` | no — `php_worker_budget` needs `route.php()` |
| `crates/blastit-view/src/php.rs:100` | `entry … must be an absolute path ([route.php] entry)` | no — the path is already resolved from `entrypoint php` |
| `crates/blastit/src/host.rs:2064` | claim label `route "app": [route.php] entry` | test text only |

`MB.21.4.10` (opened by the conductor, `depends_on: MB.21.4.5.6`) owns exactly this table and adds the
owner's rule: zero `.toml` field names anywhere in CLI or JSON output.

**(b) Doc comments on the kept-but-unreachable surface — recordable, blocked on `MB.21.5.1`**

`model.rs:309,354,614,620,625` (`StaticPolicy`/`PhpPolicy`/`Route::static_policy`/`Route::php`),
`model.rs:514,524` (`with_static`/`with_php`), `model.rs:380` (`[route.fastcgi]` on `Executor`),
`error.rs:479,486,493,502,509,512,526,543,550,557,567,574,582,589,597` (public variants naming the
per-route blocks), `validate.rs:16,43,49,57,391,535,574,580,701,728` (private items). All say "retired"
or are private; no `Display` message quotes the spelling.
The per-route `static`/`php` pair itself (`Route::static_policy`/`php`, `with_static`/`with_php`) and
`StaticDeadDeclaration`/`PhpDeadDeclaration`/`PhpWorkersInvalid` remain unreachable from a `Blastfile`;
`docs_fences`' retired-page predicate still reads the pair, so its removal is blocked on `MB.21.5.1`
rewriting the two pages.

**(c) Files outside every `MB.21.4.5*` path**

`crates/blastit-ingress/src/lib.rs:4,16,85,99,101`, `crates/blastit-dbproxy/src/config.rs:329`,
`crates/blastit-php-sapi/src/lib.rs:11`, `crates/blastit-view/src/php.rs:65,176`,
`crates/blastit-scheduler/src/{rewrite,scheduler,static_asset}.rs` comments,
`crates/blastit-sqlpolicy/src/{lib,tables}.rs` rustdoc, `crates/blastit/src/{host,config,supervise,
dev_learn,stage0,check,audit_v1}.rs` comments, `crates/blastit-dbproxy/tests/postgres.rs:673` — all
`///`, `//!` or `//`, none rendered. Recordable for `MB.21.5.1` (docs) or the `MB.21.4.10` sweep.

**(d) Deliberately kept — these are the "there is no detector" proof**

Fixtures and guards that feed retired text to the one reader and assert an ordinary
`Blastfile:<line>:<col>` syntax error, or that assert the retired name is absent:
`crates/blastit-manifest/tests/blastfile_errors.rs:58,113`,
`crates/blastit-py/src/audit.rs:3630,3638`, `crates/blastit/tests/blastfile_discovery.rs:11,41`,
`crates/blastit/tests/critical_check_engine.rs:1074`.
`crates/blastit-sqlpolicy/src/tables.rs:969-1317` and
`crates/blastit-sqlpolicy/tests/policy_module_tables.rs:462,473` are `TableGrants::to_toml` fixtures —
the writer itself is `MB.21.4.10`'s (its output reaches the report).

### Test curation (`docs/testing/AUDIT-MB-21-4-5.md`)

`make mutants-rust PKG=blastit-manifest` scoped to the three modules this epic rewrote, through the same
wrapper (`scripts/mutants-run.sh`, same cgroup cap and 90 s timeout):

| Module | Killed | Survivors |
|---|---|---|
| `src/validate.rs` + `src/error.rs` | 77.2 % (125/162) | 36 (+1 timeout, inconclusive) |
| `src/routing.rs` | 77.3 % (17/22) | 5 |
| `src/blastfile/lower.rs` | 93.0 % (106/114) | 8 |

All three clear the ≥ 75 % bar. 150 protected cases across 15 `critical_*` suites; every sampled security
rule asserts both the allowed and the denied case; zero tautological or broken-oracle tests; one
deliberately narrowed (`docs_fences`, above) and one stage-agnostic (`refusals()`, protected).

Gaps filed for **`MB.21.5.3`** (the milestone's own mutation audit), not as new leaves here: (1)
`ManifestError::position()` and `blastit fmt`'s single-prefix `./Blastfile:2:10:` spelling are unread by
any test; (2) the php entry's `byte < 0x20` upper bound is unpinned (no accepted entry contains a space);
(3) `root`/`index`/dotfile rules are implemented twice (`lower.rs:912,930,935,937` and
`validate.rs:419,420,450`) and pinned only at their union; (4) `RoutingTable::methods_for` has no test in
`blastit-manifest` (covered only by `blastit-scheduler/tests/critical_blastfile_parity.rs:755`); (5) the
dead validate walks enumerated below; (6) `lower.rs::pg_grant`'s unreachable `Clause::Query` arm; (7) a
two-word grant header (`postgres db extra { … }`) is untested, so `store.is_none()` is unpinned; (8)
`for_learn`'s `+1` shift is not tied to `stage1`'s `RouteInfo` prepend anywhere.

### Gate refinements and carried work

- **The epic's acceptance item 2 command over-matches.** `git grep -n 'mod parse' crates/` prints
  `crates/blastit-manifest/src/blastfile/mod.rs:47:mod parser;` — the retained Blastfile parser, not the
  retired module. The anchored form `git grep -nE '^mod parse;' crates/` is empty. The command is recorded
  here rather than retro-edited so the gate text is not quietly weakened; `MB.21.4.5.6`'s own acceptance
  already uses the anchored form.
- `Proxy::output` (`crates/blastit-manifest/src/model.rs`, rendered by `blastit-env/src/{postgresql,env}.rs`)
  and `TableGrants::to_toml` (`crates/blastit-sqlpolicy/src/tables.rs`, called from
  `crates/blastit-py/src/audit.rs`, asserted in `crates/blastit-sqlpolicy/tests/critical_policy_surface.rs`
  and `python/blastit/pytest_plugin.py`) survive by design — `MB.21.4.10`.
- `crates/blastit/src/cli.rs::one_line` is a second name for `blastit_manifest::clipped`; `lib.rs`'s
  "needed once every route is static" module sentence is stale (out of this epic's paths).
- `Location::Group` and `Location::Trait` render identically; one-token cleanup if they are unified.
- `cargo test --all-targets --doc` is rejected by cargo ("can't mix --doc with other target selecting
  options") — worth a `docs/knowledge/LESSONS.md` line from a node that owns it.
- `MB.21.5.1` owns the docs sweep (the eight retired pages, `docs/reference/manifest.md`,
  `docs/ci.md`'s `--json` values, `docs/reference/cli.md:95`), and `MB.21.5.2` the strict single-opener
  `docs_fences` plus the retired-name guard — this epic narrows `docs_fences` deliberately in the
  meantime (counted, named, not parsed) and records it in `MB.21.4.5.3`'s RESULT.
