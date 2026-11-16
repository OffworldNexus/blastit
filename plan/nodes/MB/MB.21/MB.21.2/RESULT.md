# RESULT — MB.21.2

## Summary

Wrote the technical design of the Blastfile language and its ADR from the two
accepted DX pages (MB.21.1), the settled spec in MB.21 and the code that must keep
behaving, then ran four fresh-context goldfish critics (`security`,
`implementability`, `testability`, `consistency`) over four rounds and resolved
every BLOCKER/MAJOR finding in the document. By the owner's decision no fifth round
was run: the review files are pinned at the round each was written in, and the lead
accepts on the resolution ledger in `## Review Resolution` and design §22.

* `docs/design/blastfile-dsl.md` — **1959 lines**, 22 numbered sections: the DX
  target (§1), goals/non-goals (§2), 29 testable requirements (§3), crate home,
  pipeline, model changes, public surface and budgets (§4), then one section per
  pinned item — tokens/lexer (§5), LL(1) grammar + FIRST sets + the view decision +
  the refusal table (§6), the v1 capability map row by row (§7), the path-pattern
  table and `HEAD` (§8), static mounts and the four-step precedence (§9),
  `body raw` / `body raw unvalidated` (§10), traits (§11), entrypoints and the
  `workers` removal (§12), grant mapping and the `fix.to` shapes (§13),
  CST/`fmt`/surgical editor with P1–P4 (§14), **discovery and the erasure of the
  retired format (§15)**, docs fences and the count-parity rule (§16), security
  (§17), the error contract (§18), alternatives (§19), the R→test table (§20), the
  ordered leaf plan against the real plan ids (§21) and the review resolution
  (§22). Every section names its code site and its test tier.
* `docs/decisions/ADR-0012-blastfile-dsl.md` — 14 decisions (LL(1) by hand, zero
  dependencies, bounded input, lossless CST, no model rename, the two `raw` forms,
  the `workers` removal, mounts before views, `HEAD` at parse time,
  `$IDENT`-is-a-name, one-line errors, the fence opening, the crate home, and
  **the total erasure of the retired TOML format** with its guard), the
  trade-offs, the refused alternatives and the falsifiers.
* `docs/decisions/README.md` — the `ADR-0012` row, status `Accepted`, node `MB.21`.

The four owner decisions are recorded and not re-opened: `body raw` /
`body raw unvalidated` as a `body` sub-token (§10, ADR decision 6), `workers`
removed with its reason (§12.3, ADR decision 7), zero dependencies (§19.1, ADR
decision 2), and `fix.*` re-spelled to Blastfile nodes (§13.3). The owner's
mid-flight erasure directive replaced node item 10: §15 now pins the **negative**
(no detector, no second reader, no converter, no migration sentence, no
dependency, no mention), with R27–R29 and a guard in the existing
`python/tests/test_docs_assets.py`.

## Evidence

### Files exist, ADR registered

```console
$ ls docs/design/blastfile-dsl.md docs/decisions/ADR-0012-blastfile-dsl.md
docs/design/blastfile-dsl.md
docs/decisions/ADR-0012-blastfile-dsl.md
```

```console
$ grep -n 'ADR-0012' docs/decisions/README.md
18:| [ADR-0012](ADR-0012-blastfile-dsl.md) | The Blastfile DSL: LL(1) grammar, no header, zero-dependency lexer/parser, lossless CST, and the `workers` removal | Accepted | MB.21 |
```

### Critic verdicts (pinned at their rounds)

```console
$ grep -rn 'VERDICT' docs/design/blastfile-dsl.review-*.md
docs/design/blastfile-dsl.review-consistency.md:459:VERDICT: FAIL
docs/design/blastfile-dsl.review-implementability.md:329:VERDICT: FAIL
docs/design/blastfile-dsl.review-security.md:443:VERDICT: FAIL
docs/design/blastfile-dsl.review-testability.md:306:VERDICT: FAIL
```

Four files, pinned at the round each review was written in; each therefore still
reads `VERDICT: FAIL`. No fifth round was run, by the owner's decision: every
BLOCKER/MAJOR those files raise is resolved in design §22 (accepted with the
section that changed, or rejected with the owner decision as the reason), and the
**resolution ledger is accepted by the lead**, verified by a fresh goldfish
`reviewer` that opens §22 and the cited sections. The consistency round-4 file
(459 lines, 3 BLOCKER / 13 MAJOR / 6 MINOR) is the last one written; its findings
landed as: the guard is **one** pytest with no allowlist, `fold` keeps its
required `--manifest PATH` (`ADR-0002`), the `fix` action names stay the accepted
page's v1 set, and Python ownership is `MB.21.4.3`'s while `MB.21.4.5` is
Rust-only.

### Plan and site gates

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
```

### Size

```console
$ wc -l docs/design/blastfile-dsl.md docs/decisions/ADR-0012-blastfile-dsl.md
  1959 docs/design/blastfile-dsl.md
   211 docs/decisions/ADR-0012-blastfile-dsl.md
  2170 total
```

### The erasure is stated as a testable rule

```console
$ grep -n 'no format detection\|retired file name appears nowhere\|the_retired_format_is_gone' docs/design/blastfile-dsl.md | head
110:| R16 | **There is no format detection, no fallback format and no migration path.** …
122:| R27 | After the cutover, the retired file name appears nowhere in the **strict set** …
1246:`python/tests/test_docs_assets.py::the_retired_format_is_gone` (run by
```

## Files

| Path | What |
|---|---|
| `docs/design/blastfile-dsl.md` | the technical design, 1959 lines |
| `docs/design/blastfile-dsl.review-security.md` | `security` goldfish review |
| `docs/design/blastfile-dsl.review-implementability.md` | `implementability` goldfish review |
| `docs/design/blastfile-dsl.review-testability.md` | `testability` goldfish review |
| `docs/design/blastfile-dsl.review-consistency.md` | `consistency` goldfish review |
| `docs/decisions/ADR-0012-blastfile-dsl.md` | the decision record, 211 lines |
| `docs/decisions/README.md` | one new row |
| `plan/nodes/MB/MB.21/MB.21.2/RESULT.md` | this report |

## Review Resolution

The four review files are **pinned at the round each was written in** and
therefore still read `VERDICT: FAIL`: no fifth round was run, by the owner's
decision of 2026-11-16 (the same call accepted for `MB.21.1`). Every BLOCKER and
MAJOR in them is resolved in `docs/design/blastfile-dsl.md` §22 — accepted with
the section that changed, or rejected with the owner decision as the reason
(`workers`, the `body raw` acknowledgement, zero dependencies, the retired
format's erasure) — and §22 also carries the findings of the two sessions that
could not persist a file. **The lead accepts on this ledger**, verified by a fresh
goldfish `reviewer` that opens §22 and the cited sections; a literal
`VERDICT: PASS` line is not part of the bar.

The complete, per-finding ledger (rounds 1–4) is §22 of the design — every row is
`accepted -> §X`, `rejected -> reason`, or `deferred -> node`. Summary:

| Lens | Round | BLOCKER | MAJOR | Resolution |
|---|---|---|---|---|
| `security` | 1 | 2 | 10 | all accepted and landed (§5.3, §4.5, §9.1–§9.4, §13.3, §17.3, §17.4, §18) |
| `security` | 2 | 1 | 5 | all accepted and landed (§4.5 `read` regular-file+`take`, §9.2 nested prefixes and the fallback method gate, §13.3 `to` as a locator, §15.2 `blastit-py` reader) |
| `security` | late write | 1 | 9 | all accepted and landed (design §22 "Late write": `MB.21.4.5` paths, `static_forward` deleted, the fallback `probe` site, `root`'s new dotfile rule, `sandbox`-keyed sockets/workers, `TRACE` on a method-less view, one `read` for every entry point, no echoed `InvalidApp`/`PhpEntryInvalid` value) |
| `implementability` | 1 | 3 | 9 | all accepted and landed (§6.1 grants, §4.1/§6.4/§14.1 one CST, §21 real leaves, §9.5 zero-view boot, §13.3 `fold`) |
| `implementability` | 2 | 5 | 1 | all accepted and landed (multiple table grants, `MB.21.3.0` model-parity leaf, endpoint blocks body-only, `blastit-py` binding, §4.5 budgets) |
| `implementability` | 3 | 3 | 8 | all accepted and landed (§21 leaf sizing/split, path ownership, guard in `test_docs_assets.py`) |
| `testability` | 1 | 6 | 10 | all accepted and landed (§4.3 `equivalent`, §14.3 P2b, three fence classes, §20 oracle rewrites) |
| `testability` | 2 | 3 | 10 | all accepted and landed (§21 ownership, inventory row, guard sets, corpus floor, lint placement) |
| `testability` | 3 | 3 | 6 | all accepted and landed (§21 names/paths, guard home, pre-migration gates, P1/P3/P4 arity) |
| `consistency` | 1 | 3 | 11 | all accepted and landed (§6.1, §9.2, §13.3, §16, §21) |
| `consistency` | 2 | 2 | 9 | accepted where the text still carried them; both BLOCKERs were already fixed by the round-2 grammar/precedence edits (endpoint blocks body-only, fallback after views) |

Two critic sessions could not persist their file (one hit its step limit, one hit a
write permission refusal). Their findings are reproduced and resolved above and in
§22, so no finding is lost; the files on disk are the reviews that did land.

## Follow-ups

The epic owner runs these **before** `MB.21.3` starts. Each names the node and the
exact text or paths to write; all of them are also in design §21.

1. **(a) Create `MB.21.3.0`** — title: `Model parity for the Blastfile cutover:
   Manifest::name/mounts/php/equivalent, StaticMount, StaticPolicy::fallback,
   Route.sandbox, MountTable, uid by sandbox`. `depends_on: [MB.21.2]`; parents
   `MB.21.3`; owner `implementer`. `paths`:
   `crates/blastit-manifest/src/{model,routing,uid,validate,error,parse,edit}.rs`,
   `crates/blastit-manifest/tests/{routing,validate,uid,parse,critical_manifest_011,critical_manifest_edit,critical_manifest_edit_executor,edit_branches,grants,critical_static_manifest,critical_php_manifest,docs_fences}.rs`,
   `crates/blastit-scheduler/src/{route,rewrite,static_asset,static_response}.rs`,
   `crates/blastit-scheduler/tests/{critical_rewrite,critical_static_entry}.rs`,
   `crates/blastit/src/{config,stage0,stage1,check,audit_v1,dev_learn}.rs`,
   `crates/blastit-php/src/**`. If the diff exceeds one session, split into
   **`MB.21.3.0a`** (that first src group + the manifest-crate tests) and
   **`MB.21.3.0b`** (the scheduler, `blastit` and `blastit-php` groups), each
   keeping `make check` green.
2. **(b) Create `MB.21.4.5`** — title: `Erase the retired manifest format from
   Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and
   every mention`. `depends_on: [MB.21.4.3]`; owner `implementer`. `paths`:
   `crates/blastit-manifest/src/parse.rs` (deleted),
   `crates/blastit-manifest/src/model.rs`, `crates/blastit/src/stage1.rs`,
   `crates/blastit-manifest/Cargo.toml`, `Cargo.toml`, `Cargo.lock`.
3. **(c) Widen each leaf's `paths`** (design §21) with `plan.py edit`:
   * `MB.21.3.1` += `crates/blastit-manifest/src/lib.rs`,
     `crates/blastit-manifest/src/blastfile/{mod,tokens}.rs`,
     `crates/blastit-manifest/tests/{blastfile_errors,critical_blastfile_errors}.rs`,
     `scripts/{check_manifest_purity,test_manifest_purity}.py`.
   * `MB.21.3.2` += `crates/blastit-manifest/tests/{blastfile_traits,blastfile_grants,blastfile_body,blastfile_entrypoint,critical_blastfile_parity,critical_blastfile_traits,critical_blastfile_body,critical_blastfile_entrypoint,critical_blastfile_security}.rs`,
     `crates/blastit-manifest/tests/fixtures/**`.
   * `MB.21.3.4` += `crates/blastit-manifest/tests/{blastfile_fmt,critical_blastfile_print}.rs`,
     `crates/blastit/tests/{fmt_command,critical_blastfile_fmt}.rs`.
   * `MB.21.4.1` += `crates/blastit/tests/{blastfile_discovery,critical_config_and_presets}.rs`.
   * `MB.21.4.2` += `crates/blastit-scheduler/tests/{critical_static_entry,critical_static_assets}.rs`,
     `crates/blastit-manifest/tests/critical_static_manifest.rs`,
     `crates/blastit/tests/critical_raw_ingress.rs`.
   * `MB.21.4.3` += `crates/blastit-py/src/{lib,audit}.rs`,
     `python/blastit/{fold,pytest_plugin,__init__}.py`,
     `python/tests/{test_check,test_fold}.py`.
   * `MB.21.5.1` += `AGENTS.md`, `.opencode/**`,
     `python/tests/runnable_docs/inventory.toml`, `zensical.toml`.
   * `MB.21.5.2` += `python/tests/test_docs_assets.py`.
4. **(d) Re-title `MB.21.4.1`** from
   `Discover ./Blastfile and refuse blastit.toml/TOML input with one migration
   sentence at every entry point` to
   `Discover ./Blastfile (BLASTIT_MANIFEST, no upward search) and fail a missing
   file with the plain missing-file sentence — there is no format detection and no
   migration`. Its acceptance loses every "migration sentence" clause and gains:
   "a file that is not a Blastfile fails with the ordinary
   `Blastfile:<line>:<col>` syntax error".
5. **(e) Re-word two briefs**:
   * `MB.21.3.2`'s acceptance item that says "with the v1 model-equality oracle"
     becomes "with the expected `Manifest` built in the test for each converted
     fixture; the rows the design drops are asserted absent".
   * `MB.21.4.3`'s `# Goal`/`# Acceptance` that prescribe `add_trait` / `add_view`
     / `add_body` become `add_group` / `add_route` / `declare_ingress` (the
     accepted page's closed action set, design §13.3), and its arrow locator
     `view "restaurants" → postgres "db"` becomes `view restaurants postgres db`.

Leaf-level notes already carried by §21, not separate planner actions: `fold`
keeps its required `--manifest PATH` (now a Blastfile) per `ADR-0002`;
`MB.21.5.1` fixes the reference's `$IDENT`-echo row and the `--learn`/`body`
contradiction; `MB.21.4.3` adds the `blastfile_apply_edits` PyO3 binding;
`MB.21.3.1` routes `check.rs`'s read through `blastfile::read`; `MB.21.5.2`
removes the `blastfile` fence alias at the strict flip.
