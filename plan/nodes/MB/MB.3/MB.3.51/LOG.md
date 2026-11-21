# Log MB.3.51

## 2026-11-15T07:46 [lead]

created (task) Does check --learn fulfil the single-pass promise? Decide (a) restore it, or (b) amend VISION, and rule on MB.3.19's mismatch inventory

## 2026-11-15T07:46 [lead]

status ready -> active

## 2026-11-15T07:46 [lead]

attempt 1/3 started

## 2026-11-15T07:46 [lead]

base 872f523

## 2026-11-15T08:04 [lead]

status active -> review

## 2026-11-15T08:08 [lead]

REVIEW FAIL (attempt 1). DECISION (a) is accepted and all four of your fact corrections hold (the reviewer re-verified each in code, including `[route.db]` being correct TOML). RULING 4 was independently CONFIRMED fail-open and has already been escalated: MB.3.53 (priority 0, release-blocking) is implementing it now, so keep the ruling and do not re-open it.
Three MAJORs in the record, all "the record would misfire if implemented as written":
MAJOR 1: `learn-single-pass.md:485-487` — the ci.md row leaf 6 copies ("exits 0 when there is nothing left to write … and 1 … when it wrote nothing") contradicts §5.5 row 2 and ci.md:121's own `--learn --dry-run` (writes nothing, exits 0). Replace with: "`--learn` exits `0` for a fixed point or a `--dry-run`, and `1` with the reason on stderr when it was blocked (a refusal, a write failure, or a suite that reached nothing)."
MAJOR 2: `:1076` leaf 8 lists `crates/blastit-manifest/src/validate.rs`, but §7.1:657-703 puts the rule at stage0/stage1/check; a validate.rs rule would make `parse_and_validate` reject `app` + `spa = true`, breaking the PROTECTED `crates/blastit-manifest/tests/critical_static_manifest.rs:524-537` ("the page plus an app boots"). Drop validate.rs from leaf 8, keep model.rs only, and name `stage0.rs:5061-5080` (`the_two_halves_of_the_rule_still_hold`) as a deliberate in-scope change with the assertion that must move.
MAJOR 3: `:242-245` — the "a redacted statement" refusal prints the wire `normalized` UNREDACTED, contradicting R19/R12; the security round-3 item 1 is absent from §13 while your RESULT claims every finding was graded. Print the label only or `redact_literals(&normalized)`, and grade the finding in §13.
MINORs to fix or explicitly grade (do not silently drop): `:408-410` says `critical_check_engine.rs:1169` asserts `wrote == true` — it asserts `false`; `:883-884`'s "`Vec<bool>` is bit-packed … ≈ 1 KiB" is false (1 byte/elem, ≈ 8 KiB) and §7.3:791's "~1 M comparisons" ignores `MAX_PATTERNS = 32` (validate.rs:52) → ≈33.5 M, and add the round-3 early exit; the `static_asset.rs` line citations have drifted (56/43/70-73/172-178/198-226); §13:1090 says "two rounds each" but §13.5 records 12; §6.1:536 uses bare `python3 -c` (repo rule: uv only); leaves 8/9 name no writable test path though §7.1:703 and §11:1047 promise one; §5.4:326-330 records no §13 entry for the `"""`-entry comma case; and the ordering must sequence leaf 10 before leaf 2 (both edit `audit_v1.rs`, and leaf 2's hint text names `set_transaction_control`, which only leaf 10 creates) or drop that parenthetical.
Do NOT open another critic round: fix these, re-run `make docs-check` + `plan.py validate`, update RESULT.md's grading table so it matches §13, and report.

## 2026-11-15T08:08 [lead]

status review -> ready

## 2026-11-15T08:08 [lead]

status ready -> active

## 2026-11-15T08:08 [lead]

attempt 2/3 started

## 2026-11-15T08:10 [lead]

status active -> review

## 2026-11-15T08:12 [lead]

REVIEW FAIL (attempt 2): three MAJORs from attempt 1 are CLOSED and verified (exit-code row, leaf 8 no longer naming validate.rs so the protected app+spa fixture still boots, label-only refusal with the security item graded in §13). One MAJOR remains, plus four MINORs — all internal inconsistencies of the same family:
MAJOR: `:1091` (also `:620`, `:1202`) — leaf 3 owns "the one `plan.json(false)` call … `git diff` on the protected file shows only the call site", contradicting §5.5:415-423 and §11:1062 ("four … :1111, :1141, :1169, :1183 … exactly four changed call expressions and no changed assertion"). Implemented as written, `:1183`'s `wrote == true` fails after the `json(wrote)` swap and `:1112/:1142/:1170` stay tautological. Change all three places to: "the four `plan.json(false)` call expressions (`:1111`, `:1141`, `:1169`, `:1183`), all `plan.json(plan.would_write(false))`, no assertion changed" and drop the "one call site" acceptance wording.
MINOR 1: `:453-458` — the four-state table's key is only (`proposed`, `wrote`) and rows 1/2/3 overlap, so "read the first row that matches" predicts exit 0 for a blocked run with `proposed 0,0` (zero observations, or one refusal and nothing grantable), where `check.rs:984-1004` returns 1. Add a `cause`/`--dry-run` column.
MINOR 2: `:1097` — leaf 9 says it creates `ADR-0011-raw-ingress-production-gate.md`, but MB.3.53 has already created `docs/decisions/ADR-0011-raw-ingress-boot-refusal.md` (it is in the tree now). Say "ADR-0011 already records it (MB.3.53); leaf 9 creates nothing".
MINOR 3: `:211` (with `:159`) — §5.2's normative `format!` interpolates `{redacted}`, `{reason}` and a bridge-supplied `proxy` with no escaping or cap, while R19 requires control-byte escaping and a 2048-byte cap on every component. Name the escaper/cap in the pasted block or state that the leaf applies R19 to each component.
MINOR 4: `:1100-1105` — "8 and 9 both edit stage0.rs/stage1.rs (8 then 9)" is already inverted in this tree: MB.3.53's raw-ingress gate (leaf 9) is uncommitted and present, so leaf 8 must land AFTER it. Say "9 has landed as MB.3.53; leaf 8 rebases on it".
This is attempt 3; no new critic round, no reshape beyond these five points. Report with the final order.

## 2026-11-15T08:12 [lead]

status review -> ready

## 2026-11-15T08:12 [lead]

status ready -> active

## 2026-11-15T08:12 [lead]

attempt 3/3 started

## 2026-11-15T08:14 [lead]

status active -> review

## 2026-11-15T08:21 [lead]

REVIEW FAIL (attempt 3): four of five points PASS, including the four-call-site wording, leaf 9/ADR, §5.2 caps and the 9 -> 8 order. ONE MAJOR remains, and it is the same table as attempt 2, so this is the last cheap round — attempt 4 is restricted to three edits and nothing else.
MAJOR (docs/design/learn-single-pass.md:462-467): row 2 (`no | 0,0 | false -> 0`) still shadows row 4 for the blocked cases row 4 names (zero observations; a refusal with nothing grantable), so the "read the first row that matches" rule at :458 predicts exit 0 where `clean` (`check.rs:991`) returns 1 — and §5.5:446-449 itself says the zero-request run prints {"proposed":{"routes":0,"queries":0},"wrote":false} and exits 1. Use the reviewer's fix verbatim: add a `clean` column keyed on `plan.requests > 0 && plan.refused.is_empty() && failure.is_none()` (`check.rs:991`), row 2 `clean = yes`, row 4 `clean = no` — OR delete row 2 and write "`0,0` + `wrote:false` is exit 0 only when clean".
MINOR 1 (:424-427): "leaves four call sites" then enumerates five (`audit_v1.rs:1456` plus `:1111`, `:1141`, `:1169`, `:1183`). Say "leaves five call sites — the bridge's own at `:1456` and the four protected ones, all `plan.json(plan.would_write(false))`".
MINOR 2 (:1092 and the same omission at :628-635): "Four protected files are touched" / "Two … A third … One" omits `crates/blastit/tests/critical_dev_and_fail_closed.rs`, which leaves 8 and 9 add to under `critical_guard.py`'s `crates/*/tests/critical_*.rs` glob. Name it in both inventories.
Cosmetic, fix if trivial: `critical_manifest_011.rs:450` should point at `:459` for the `RawIngress` assertion.
These three edits only. No other section, no reshape, no new critic round. Then `make docs-check` + `plan.py validate` and report.

## 2026-11-15T08:21 [lead]

status review -> ready

## 2026-11-15T08:21 [lead]

status ready -> active

## 2026-11-15T08:21 [lead]

edited fields

## 2026-11-15T08:21 [lead]

attempt 4/4 started

## 2026-11-15T08:22 [lead]

status active -> review

## 2026-11-15T08:26 [lead]

Reviewer PASS on attempt 4 with one MINOR accepted as a recorded residual rather than a fifth attempt: `:426-427` says "all `plan.json(plan.would_write(false))`" in a clause that grammatically reaches the bridge's own call at `audit_v1.rs:1456`, whose correct replacement is `plan.json(plan.would_write(dry_run))` (§13:1224 already scopes "all" to the four protected calls, so the record's normative statement is right and only the sentence's grammar overreaches). Exact replacement for whoever next opens it: "the four protected ones — all `plan.json(plan.would_write(false))` — and the bridge's own becomes `would_write(dry_run)`". This is a wording fix in a design record whose leaf 3 implementer reads §13 as well; it does not change the decision and does not justify another round trip.

## 2026-11-15T08:26 [lead]

status review -> done
