# Result MB.2

## Summary

Static serving and clean URL rewriting ship in `0.2.0` in the DX-first order the epic requires:
the page (`docs/frameworks/static-and-spa.md`, 147 lines) and its two critic lenses closed first,
then `docs/design/static-and-spa.md` + `ADR-0007` through three more lenses, then manifest
grammar → resolver/responder → rewrite/dispatch → `dev`/`check` → e2e, with five protected
`critical_*.rs` suites owning the traversal, NUL, symlink-escape, dotfile and immutable-cache
oracles. All 18 nodes closed at `14a52a3` (9 top-level tasks, 2 epics, 7 nested leaves); the gate
audit filed three gaps and all three are closed, two of which were real product bugs.

## Evidence

| # | `# Acceptance` (NODE.md) | Status | Command / artefact | Source |
|---|---|---|---|---|
| 1 | Lean user documentation written and accepted by `dx` and `ai-ergonomics` critics **before** internal design or Rust | **met** | `tail -1 docs/design/static-and-spa.review-{dx,ai-ergonomics}.md` → `VERDICT: PASS` (14 + 20 findings, every BLOCKER/MAJOR resolved); the design (`MB.2.2`) started only after that PASS and then passed `security`/`implementability`/`testability` | `MB.2.1/RESULT.md`, `MB.2.2/RESULT.md`, the two `docs/design/static-and-spa.review-*.md` artefacts |
| 2 | Built-in static serving and SPA / front-controller rewriting with protected `critical_*.rs` positive/negative tests (traversal, NUL, symlink escape, dotfile denial, immutable hashed caching) | **met** | `cargo test -p blastit-manifest --test critical_static_manifest --locked` → `15 passed`; `-p blastit --test critical_static_cli` → `12 passed`; `-p blastit-scheduler --test critical_static_resolve` → `18 passed`, `--test critical_static_assets` → `14 passed`, `--test critical_rewrite` → `32 passed` (re-run at `14a52a3` for this gate; 91 protected tests, 0 failed) | `MB.2.3`–`MB.2.5` RESULTs; `docs/design/static-and-spa.md` §5.2 (40-row matrix × `Openat2`/`Walk`), §7.2 oracle map (`MB.2.10`) |
| 3 | `blastit dev`, `blastit check` (`--learn` / `--json`), runnable-docs fences, `zensical build --strict`, `make check`, `make e2e`, epic `test-curator` audit | **met** | `make check` exit 0, `47 fences: 28 executed, 19 accounted, 0 failed` + `plan OK`; `make e2e` exit 0 (356 deployment `ok`, 72 static assertions, `exit 0 wall 31s`); `make docs-check` → `No issues found`; `make docs-test` → `0 failed`; audit `PASS`, no BLOCKER | `/tmp/opencode/mb226-make-check.log`, `/tmp/opencode/mb211-e2e.log`, `MB.2.7/RESULT.md`, `docs/testing/AUDIT-MB.2.4.md`, `docs/testing/VISUAL-MB.2.md` |

Bullet 1 was satisfied by `MB.2.1`'s two critic lenses (page accepted before `MB.2.2` opened) and
`MB.2.2`'s review order (design reviewed by `security`/`implementability`/`testability` before any
Rust); bullet 3 by the gates above, run on the `0.2.0` tree after the version bump.

### Gate checks beyond the acceptance block

| Check | Result | Source |
|---|---|---|
| `make check` (final tree) | exit 0, 55.7 s, `47 fences: 28 executed, 19 accounted, 0 failed`, `plan OK` | `/tmp/opencode/mb226-make-check.log` |
| `make e2e` | exit 0 at the `0.2.0` tree; the bump made the harness refuse two wheels in `dist/` (`found 2`), stale wheel removed, re-run clean; `docs_deploy: 40/40` in the log, 41/41 after `MB.2.11`'s fixes | `/tmp/opencode/mb211-e2e.log`, `MB.2.8/RESULT.md` |
| e2e static leg | 72 assertions against a real containerised `blastit dev`: immutable asset `200`, deep SPA URL `200 text/html`, `400` for `..`/`%2e%2e`/NUL/backslash, `404` dotfiles + out-of-root symlink (never file bytes, never the fallback), `HEAD`, `405` + `Allow`, `413`, `spa = false` `404` | `MB.2.7/RESULT.md`, `docs/testing/VISUAL-MB.2.md` (§3, `VERDICT: PASS`) |
| Docs gates | `make docs-check` → `No issues found`; `make docs-test` → `47 fences: 28 executed, 19 accounted, 0 failed`; `make docs-unit` → 63 passed, 57 subtests | `MB.2.9/RESULT.md`, `MB.2.8/RESULT.md` |
| Release gates | `scripts/check-release-tag.sh v0.2.0` → "tag v0.2.0 names the workspace version 0.2.0"; `scripts/release-notes.sh --require-dated v0.2.0` → dated entry present; `cargo metadata --locked --no-deps` OK, 28/28 intra-workspace requirements at `0.2.0`; `CHANGELOG.md` has `## 0.2.0 (2026-11-14)` | `MB.2.8/RESULT.md`, `CHANGELOG.md` |
| Epic `test-curator` audit | **PASS** on tier, teeth and fixture hygiene; `critical_rewrite.rs` "deserves protected"; 96 mutants in `static_asset.rs` = 68.6 % raw / 88.1 % after 19 equivalences; 8 meaningful survivors filed as `MB.2.4.3`/`.4`/`.5` and **all closed** | `docs/testing/AUDIT-MB.2.4.md` (§Verdict, §GAPS) |
| Mutation campaigns | resolver `static_asset.rs` 66/86 = 76.7 % raw (66/66 after equivalence classes); responder `static_response.rs` 152/159 = 95.6 % viable | `MB.2.4.3/RESULT.md`, `MB.2.4.4/RESULT.md` |
| Preset fail-closed | `blastit --preset digitalocean-paas` / `compose-traefik` still exit `1` without capabilities; `docs_deploy: 41/41` deployment promises hold (pooler URLs, direct migrate, role-default timezone) | `MB.2.11/RESULT.md`, `e2e/docs_deploy.py`, fence mode `presets` |

## Files

Delivered under this epic (child RESULTs hold the per-node file lists):

| Area | Artefacts |
|---|---|
| DX page | `docs/frameworks/static-and-spa.md`, `docs/frameworks/index.md`, `zensical.toml`, `python/tests/runnable_docs/inventory.toml`, static-site fixture |
| Design | `docs/design/static-and-spa.md`, `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md`, five `*.review-*.md` artefacts |
| Manifest | `crates/blastit-manifest/src/{model,parse,validate,error,lib}.rs`, `tests/critical_static_manifest.rs`, `tests/docs_fences.rs`, `tests/parse.rs` |
| Scheduler | `crates/blastit-scheduler/src/{static_asset,static_response,rewrite,route,lib,config}.rs`, `scheduler.rs`, `tests/critical_static_{resolve,assets}.rs`, `tests/critical_rewrite.rs`, `tests/common/static_fixture.rs` |
| CLI / boot | `crates/blastit/src/{config,stage0,stage1,check,audit_v1}.rs`, `tests/critical_static_cli.rs` |
| e2e + visual | `e2e/static_assets.py`, `e2e/static_site/`, `e2e/compose.yaml`, `e2e/run.py`, `e2e/docs_deploy.py`, `docs/testing/{VISUAL-MB.2,AUDIT-MB.2.4}.md` |
| Public docs | `docs/security.md`, `docs/how-it-works.md`, `docs/ci.md`, `docs/reference/{cli,manifest,settings}.md`, `docs/deploy/{index,digitalocean-paas,compose-traefik,anything-else}.md`, `docs/research/connection-pooling.md` |
| Release | `Cargo.toml` + 8 crate manifests (28 requirements), `Cargo.lock`, `CHANGELOG.md`, `ARCHITECTURE.md` §2.4 |

## Follow-ups

1. **Mutation residue (recorded, accepted).** Resolver: 20/86 survivors are 12 disjoint-bit `|`→`^`
   compositions, 7 `|`→`&` flag degenerations with no unprivileged oracle, and `541:5`
   `supports_openat2 → true` — only a seccomp-filtered `ENOSYS` e2e job can kill the last one
   (`MB.2.4` F1). Responder: 7 survivors — `218:41` `trim_ows` (`&&`→`||`, tab-only OWS) and six
   `civil_from_days` arithmetic mutants (`301:17`, `305:42`, `305:64`, `313:21`) with no
   `2000-02-29`/`1900-03-01` vector; **no node was ever filed** despite `MB.2.4.4`'s review asking
   (`MB.2.4` F2). Cheap: one table in `critical_static_assets.rs`.
2. **No `cargo-mutants` campaign for `rewrite.rs` or the static path of `scheduler.rs`.** Design
   §7.2 asks for `-f rewrite.rs` at ≥ 75 % killed; the epic has hand-mutations only (9 `decide`
   mutants, the slot-bound and pump mutants). `-f '**/src/rewrite.rs'` then
   `-f '**/src/scheduler.rs'` once the tree settles (`MB.2.5` F1). The chunk-0 mutant is now
   `CAUGHT`, so a future `TIMEOUT` there is a regression.
3. **The `want == 0` pump branch has no in-suite test** (`MB.2.5` F2): it can only reach `0` with a
   degenerate constant, so it is proven by mutating `STATIC_CHUNK_BYTES = 0` (13 failed with the
   guard, `EXIT: 124` without), not by a shipped-constant test; `on_static_read` also trusts
   `read.want` (F3). An injectable chunk size or a `want` assertion closes both.
4. **Protected-tier classification of `MB.1`'s policy oracles — two audits, same verdict, no node.**
   `docs/testing/AUDIT-MB.1.md` (`F1`–`F10`) and `docs/testing/AUDIT-MB1.md` (`G1`–`G7`) both say
   **PASS at the epic gate, no BLOCKER**, but a widening mutant (`is_placeholder_list -> true`,
   making a grant for `IN ($?)` allow `IN ('x')`) passes every protected sqlpolicy suite and is
   caught only by the unprotected `policy_module_edges.rs`. File a node to promote
   `policy_module_edges.rs`, (a subset of) `policy_module_tables.rs` and `audit_group_hint.rs`
   before the `MB` milestone gate.
5. **Page nits left by three accepted reviews** (all one-clause, none blocking):
   `MB.2.1` closed with 5 MINOR follow-ups (`LOG.md`, PASS attempt 2 — stale critic anchors,
   the `manifest.md:13` `app` row, `MB.2.6`'s page line reference); `MB.2.11`'s final pass is
   0/0/3 (`dx`) and 0/0/6 (`ai-ergonomics`), whose residue is the DO fill-list
   `REPLACE_WITH_THROWAWAY_SECRET`, `-v ON_ERROR_STOP=1` on the DO heredoc, the missing
   `doctl apps logs`/`docker compose logs` line, the dead `ca_cert` knob beside `sslmode=require`,
   and `psql` as an unstated deploy prerequisite; `MB.2.9` closed its 5 accuracy findings in
   attempt 2 and leaves 2 notes — one uncaptured `docs-test` `1 failed` run (name the fence in the
   summary line) and the unexecuted `curl -i` fences now covered by `e2e/static_assets.py`.
6. **README-rewrite hazard observed today.** An interrupted `scripts/build-wheel.sh` left
   `README.md`'s brand links absolute — the profile `MB.2.8` already saw at its base
   (`MB.2.8` F3, "`README.md: already absolute`"); the lead restored the file. Worth a
   `docs/knowledge/LESSONS.md` bullet and a stronger trap than the script's own idempotence check —
   e.g. `build-wheel.sh` writing and restoring from a temp copy, or a gate that fails on a dirty
   `README.md` after a wheel build. Same class: `dist/` housekeeping is manual after a version bump
   (`MB.2.8` F2 — the e2e wheel guard refuses two wheels; `build-wheel.sh` could prune stale
   `blastit-*.whl`).
7. **Documented-vs-shipped tightening.** `RESOLVE_NO_SYMLINKS` refuses **every** symlink inside a
   root, not only escaping ones, so a Capistrano-style `current/public` must name the real path; the
   page promises `404` only for a link that leaves `root` (`MB.2.2` F3). Nested immutable output
   needs one pattern per depth (`/assets/*/*`) because only `*` ships (`MB.2.2` F4). A mechanical
   `inventory.toml` reason→assertion check needs `python/tests/runnable_docs/test_harness.py`, owned
   by no leaf (`MB.2.2` F6).
8. **`BLASTIT_SCRIPT_NAME` + static routes is a real gap** (`MB.2.5` F5): `begin_raw` runs
   `head_segments` on the raw target, so under a mount prefix `/mount/assets/x.js` resolves as
   `root/mount/assets/x.js` while `docs/reference/manifest.md` says the prefix is stripped first. No
   test covers a prefix + static route — strip before `head_segments`, or document `/`-only roots.
9. **Recorded budgets are below what landed** (`MB.2.5` F4): `scheduler.rs` +875 changed lines
   against 600, `critical_rewrite.rs` 1,718 against design §8's 1,600 (which still says 1,365).
   Raise both to ~900 / ~1,750 so no leaf is told to cut a §7.2 case.
10. **Smaller residue with an explicit owner gap:** `ARCHITECTURE.md` §7.1's scheduler row still omits
    the read beneath each `root` (`MB.2.8` F1); the production-only dropped-to uid probe leg is
    unexercised by any unprivileged suite, `StaticRoot::open` being the deployment authority
    (`MB.2.6` F4); the DO/spec page still says `0.2.0` "adds no deployment story" although the
    recipes now carry a pooler (`MB.2.8` F4); retired node ids `MB.2.4a`/`MB.2.5a`/`MB.2.5b` live on
    in code comments and `canned(405)` keeps the generic pair (`MB.2.10` F2–F4); `e2e/rootless` still
    connects straight to `db` (`MB.2.11` F4), and the doctl/compose harness tables that would make
    the pool bootstrap and the zone probe checked fences are missing (`MB.2.11` F1/F8).
