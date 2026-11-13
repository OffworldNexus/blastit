---
id: MB.1.10
title: Document the add_group fix action and record the MB.1.6 gotchas
kind: task
status: done
owner: "doc-writer"
priority: 2
depends_on: [MB.1.6]
paths: [docs/ci.md, "docs/design/ma-lean-dx.md", docs/knowledge/LESSONS.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T15:20"
updated: "2026-11-13T16:42"
---

# Goal

Keep the `blastit check --json` contract documentation true after `MB.1.6` added the `add_group` fix action, and record the two non-obvious gotchas that leaf paid for, so an AI agent reading `docs/ci.md` can apply a connection-setup `missing_grant` in one edit and no future leaf re-derives them.

# Context

- `MB.1.6` changed `crates/blastit/src/audit_v1.rs` so a `missing_grant` on connection-setup SQL (`SELECT set_config('TimeZone', $?, false)`, `SET`/`SHOW`) emits `{"action":"add_group","to":"[group.connection.<proxy>].queries"}` instead of a per-route `add_query`; per-route grants still get `add_query`, and `SELECT 1` must keep `add_query` (pinned by `python/tests/critical/test_check_contract.py`).
- `docs/ci.md:74-75` still enumerates the fix actions as `add_query`, `remove_query`, `add_route`, `remove_route`, `add_test`, `declare_ingress`, `fix_ingress`, `observe` — the list is now incomplete, and it is the page the DX/AI-ergonomics promise points at ("structured for AI agents to apply cleanly in one edit").
- This is a documentation-reconciliation leaf: **no Rust, no behaviour change, no new public page.**

# Acceptance

```console
# 1. `docs/ci.md` names every fix action the binary can emit, including `add_group`.
$ grep -n 'add_group' docs/ci.md        # >= 1 match, in the fix-action sentence
$ uv run --no-project python - <<'PY'
import pathlib, re, subprocess
page = pathlib.Path("docs/ci.md").read_text()
src = pathlib.Path("crates/blastit/src/audit_v1.rs").read_text()
actions = {m for m in re.findall(r'"([a-z_]+)"\s*=>|action:\s*"([a-z_]+)"', src) for m in m if m}
# print both lists and confirm every action the source can emit appears in docs/ci.md
print(sorted(actions))
missing = [a for a in actions if f"`{a}`" not in page]
assert not missing, missing
print("docs/ci.md names every emitted fix action")
PY

# 2. The group-grant sentence is actionable, not vague: it must show the manifest shape
#    and say why (one connection-setup statement fires on whichever route connects first).
$ grep -n '\[group\.' docs/ci.md

# 3. Leanness: the page does not grow a lecture.
$ uv run --no-project python -c 'import pathlib; print(len(pathlib.Path("docs/ci.md").read_text().splitlines()))'
# printed line count must not grow by more than 8 lines versus `git diff --stat docs/ci.md`

# 4. The two gotchas are recorded where the next agent looks.
$ grep -c 'allocation oracle\|second pass' docs/knowledge/LESSONS.md    # >= 1
$ grep -c 'stage-0\|app.output' docs/knowledge/LESSONS.md               # >= 1

# 5. Internal design note no longer contradicts the shipped schema.
$ grep -n 'add_query' docs/design/ma-lean-dx.md | head   # any enumerated action list must include add_group

# 6. Green docs gates.
$ make docs-check
$ make docs-unit
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- `docs/ci.md` is a public page: keep it lean (lead with the working example), one canonical fix per finding kind, no kernel/orchestrator internals.
- `docs/knowledge/LESSONS.md` is internal (excluded from the public site): one bullet per gotcha, concrete symptom first. Record **three** gotchas: (i) allocation oracles must compare two passes in separate `#[global_allocator]` worlds with a per-thread armed flag (`MB.1.6`); (ii) production stage-0 errors are `app.output` events on stdout, not stderr, so a "the message is missing" test must read the right stream on each side of the capability gate (`MB.1.6`); (iii) **`cargo-mutants` 27.1.0 writes `mutants.out/{caught,missed,timeout,unviable}.txt` + `mutants.json`, never `outcomes.txt`** — the `awk '$NF=="caught"' outcomes.txt` ratio one-liners in older plan acceptance blocks never ran, so mutation ratios must be recomputed from the per-outcome files (`MB.1.1`; `MB.1.2`–`MB.1.5` acceptance snippets are already corrected).
- Do not rename or renumber existing `fix.action` values; `python/tests/critical/test_check_contract.py` pins the ones it covers and must not be edited here (it is not in this node's `paths`).
- If the source enumerates actions in a way your snippet cannot read, widen the snippet and say so in `RESULT.md` — the contract is "docs name every emitted action", not the snippet's shape.
