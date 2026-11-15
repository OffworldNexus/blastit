# MB.3.51 RESULT — single-pass learning: decision record

## Summary

Decided **(a) restore the promise**: `blastit check --learn` must propose exact
`queries` for a statement whose target resolved to an **existing** route, behind
the refusal/dry-run/`--json` gates it already has, and the rejected middle
(“document (a) while the code does (b)”, which is where 0.2.0 stands) is named in
the record so it is not re-proposed. `VISION.md` is not touched; `dev --learn`
stays the writer for a project with no suite, not a second step for one with a
suite.

The node's six bullets were re-read in the code at `872f523`. All six hold, but
four are oversimplified and one is incomplete, and those corrections are §2.1 of
the record: `ObsTarget::Routed { route }` **already carries the route name**
(`resolve_targets`, `check.rs:912-943`), so the fix is a `match` arm and not
attribution logic; `plan_learn_json` does not resolve targets and its SQL path
insert lacks the HTTP loop's `is_none()` guard, so the bridge can propose a
duplicate `<name>-2` route (O2); the transaction hint's `[route.db]` is **correct
TOML** and this record's first draft (which "fixed" it to
`[route.<name>.<proxy>]`) was wrong — the defect is only that the hint names no
route (O3); and `apply_edits` folds a multi-line list, which is tolerable for
`dev --learn` and not for the common path (O4).

Mismatch rulings: **3** is a code bug (the `app` and `BLASTIT_APP` spellings
disagree, and the message is false when a static route can forward) — R21, a
follow-up leaf that stays out of `validate.rs` so `parse_and_validate` keeps
booting `app` + `spa = true`; **4** is docs-accurate but too weak for production —
`allow_raw` becomes a production boot gate at both boot points (R16, `ADR-0011`
handed over; independently confirmed and already being implemented as `MB.3.53`,
so the ruling is kept and not re-opened here); **6** is a code gap — `**` support
with a tokenised DP, because the recursive matcher this record first proposed was
exponential and runs per static answer.

Mismatches 1, 5 and 7 are recorded as already settled so nobody reopens them.

## Evidence

| Check | Command | Result |
|---|---|---|
| Plan tree | `uv run --no-project tools/plan.py validate` | `plan OK` |
| Docs build | `make docs-check` | `No issues found` (zensical `--strict`) |
| Goldfish review | 4 lenses, 12 rounds (dx ×3, ai-ergonomics ×2, security ×3, implementability ×4) | reviews beside the record; every finding graded in §13. No BLOCKER outstanding: implementability's round-4 pass closed both of its round-3 BLOCKERs and returned PASS on that question; security's one MAJOR (`Fix.to` self-locating) is answered by the finding's `route` field, the convention `add_query` already uses (§7.4). |
| Lead reviews, attempts 1-2 | `LOG.md` 08:08 and 08:12 entries | attempt 1: three MAJORs and every MINOR fixed or graded. Attempt 2: one MAJOR (leaf 3's "one call site") and four MINORs (exit-code table key, ADR ownership, §5.2 escaping, 9→8 ordering) fixed — see §13.5b, which is the same list. |

Review disposition (attempts 1-2; §13.5b is the authority):

| Item | Outcome |
|---|---|
| MAJOR 1 — `ci.md` exit-code row contradicted §5.5 (`--dry-run` exits `0`) | fixed — the lead's replacement sentence is in §5.6 |
| MAJOR 2 — leaf 8 named `validate.rs` (would break protected `critical_static_manifest.rs:524-537`) | fixed — `validate.rs` dropped; `stage0.rs:5061-5080` named and the moving assertion stated |
| MAJOR 3 — the redacted-statement refusal printed the wire `normalized` | fixed — label-only (§5.2, R19) and graded in §13.5 |
| MINOR — `:1169` asserts `wrote == false` | fixed (§5.5) |
| MINOR — `Vec<bool>` is not bit-packed (≈ 8 KiB); bound ignores `MAX_PATTERNS = 32` | fixed (§7.3: 33.5 M worst case + early exit) |
| MINOR — `static_asset.rs` citations drifted | fixed (`:42`, `:56`, `:70-73`, `:172-178`, `:207-226`) |
| MINOR — "two rounds each" vs twelve | fixed (§13 preamble) |
| MINOR — bare `python3 -c` | fixed (`uv run python -c`) |
| MINOR — leaves 8/9 had no writable test path | fixed (both name `critical_dev_and_fail_closed.rs`, add-only) |
| MINOR — no §13 entry for the `"""`-entry shape | fixed (§5.4 + §13.5) |
| MINOR — ordering: leaf 10 must precede leaf 2 | fixed (§12: 1 → 10 → 2 → 3 → 4 → 5 → 6, then 7, then 8, which rebases on the already-landed leaf 9 / MB.3.53) |
| Graded, not fixed | the four MINORs in §13.6 (formatting, `ObsVerdict`/`LearnVerdict` naming, the §5.2 snippet's locals, `route[0]` in §6.1) |

Attempt 2's five items (all closed, §13.5b): the four `plan.json(false)` call
expressions in leaf 3 (§5.5/§11/leaf 3 agree) · the four-state table now keys on
`--dry-run` so a blocked `proposed 0,0` run is exit `1` · leaf 9 creates no ADR
(`ADR-0011-raw-ingress-boot-refusal.md` exists, MB.3.53) · §5.2 names the
escaper and per-component caps R19 promises · 9 has landed, leaf 8 rebases after
it.

Every fact in §2/§7 carries `file:line` checked in this worktree at `872f523`
(`check.rs`, `audit_v1.rs`, `dev_learn.rs`, `edit.rs`, `model.rs`, `routing.rs`,
`validate.rs`, `stage0.rs`, `stage1.rs`, `static_asset.rs`).

## Files

- `docs/design/learn-single-pass.md` — the decision record (new).
- `docs/design/README.md` — index row for the record.
- `docs/design/learn-single-pass.review-{dx,ai-ergonomics,security,implementability}.md`
  — the goldfish rounds (new, written by the critics).
- `plan/nodes/MB/MB.3/MB.3.51/RESULT.md` — this file.

No code, no page outside `docs/design/`, no plan status was changed.

## Follow-ups

Ordered leaves for the lead to create from §12 (each ≤ 600 changed lines):

1. **Extend, don't fold** — `patch_grant` inserts element lines and splits a
   shared `]` (`edit.rs`, `edit_branches.rs`). Lands first.
2. **Attribution** — `learn_route`, the `paths` guard, located refusals, the tx
   hint, the honest `learn:` line (`audit_v1.rs`, `check.rs`).
3. **One document, two producers** — `LearnPlan::json(wrote)` + bridge refusal
   lines; four protected call expressions (`critical_check_engine.rs:1111,1141,1169,1183`),
   authorised.
4. **CLI acceptance** — §6.1 offline case (`python/tests/critical/test_check_contract.py`,
   add-only) + the PG twin.
5. **The critical suite** — new `crates/blastit/tests/critical_learn_existing_route.rs`
   with the §6.3 oracles.
6. **Docs follow the code** — `docs/ci.md`, `docs/local.md`,
   `docs/reference/cli.md`, `docs/reference/manifest.md`; the sentences are quoted
   verbatim in §5.6.
7. *(parallel)* **Mismatch 6** — `**` in the immutable matcher (tokenised DP; a
   vector added to `critical_static_resolve.rs:750`, add-only).
8. *(after leaf 9 / `MB.3.53`)* **Mismatch 3** — one
   app-reachability rule (R21).
9. *(already landed as `MB.3.53`)* **Mismatch 4** — `allow_raw` production gate
   at both boot points (R16); `ADR-0011-raw-ingress-boot-refusal.md` already
   records it, so this leaf is a docs pass, not a new dispatch.
10. *(parallel, shares `ci.md` with 6)* **Mismatch 2's residue** — `Fix::set_transaction_control`
    (R18; the protected guard splits into two arms) + the `ADR-0004` additive-action
    note.

Open questions, recorded not reshaped (§13.6): pasted-`json!` formatting,
`ObsVerdict` vs `LearnVerdict` naming, two locals in the §5.2 snippet, and the
`route[0]` index in §6.1's one-liner.

Out of scope, recorded in §13.7: `suggest_fix` inventing `raw` + `allow_raw = true`
for an unknown sample; `--learn` writing `transaction_control = true`; writing a
partial manifest when one statement is refused.

DECISION: (a) restore
