---
id: MA.3.9
title: Account for every documented snippet shape and test the docs gate
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths:
  - python/tests/runnable_docs/fences.py
  - python/tests/runnable_docs/run.py
  - python/tests/runnable_docs/inventory.toml
  - python/tests/test_runnable_docs.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T01:19"
updated: "2026-11-13T02:34"
---

# Goal

Make the runnable-docs gate account for **every** snippet shape, so no documented command can sit
outside the inventory, and give the gate its own unit tests (`F1`, `F3`, `F5`, `F9`, `F10` in
`docs/testing/AUDIT-MA.3.md`).

# Context

- `fences._FENCE` only matches `^```console` (no indentation, no info suffix, three backticks, no
  tilde). `run.check_inventory` compares the *recognised* count with `inventory.toml`, so anything
  the regex misses is neither executed nor demanded. Reproduced in the audit: `~~~console`, four
  backticks, a two-space-indented fence, ` ```console title="…"` and a ` ```bash ` block with a
  `$ ` prompt all returned `check_inventory(...) == []`.
- `fences.public_pages` walks `zensical.toml`'s `nav` only. A page under `docs/` that is not in
  `nav` is still published (`zensical build --strict` produced `site/orphan/index.html` for
  `docs/orphan.md` in the audit) but never inventoried.
- A `console` fence whose `$ ` prompts were lost yields `Fence.commands() == ()`; `check_inventory`
  is happy, and executing modes such as `mode_check` report PASS without asserting anything
  (`F2`, fixed in the `harness.py` node — do not duplicate it here).
- A stray ` ``` ` inside a body, or an unclosed fence, truncates the fence and hides the trailing
  commands. `reason = " "` accounts a fence.
- The harness has **0** `test_` functions in 2 677 lines, so none of this is covered by `pytest`.

# Acceptance

```console
$ make docs-test
$ cargo test --workspace --all-targets --locked      # unaffected
$ uv run --no-project python -m pytest python/tests/test_runnable_docs.py -q
$ env -i HOME=$PWD/tmp/docs-home PATH="$PATH" uv run --no-project python -m pytest python/tests/test_runnable_docs.py -q
```

- `fences.console_fences` recognises an indented (≤ 3 spaces) fence, `~~~`/`~~~console`, four or
  more backticks, and an info-string suffix (` ```console title="…"`), and still numbers them in
  reading order.
- A new `scan` step (called from `check_inventory`) fails on: a fence-shaped line the parser did
  not consume, a `$ ` prompt inside a non-`console` fence, a `console` fence with no closing
  marker, and a `reason` shorter than 15 characters or equal to `TODO`/`n/a`/`-`.
- The inventory page list is `README.md` plus every `docs/**/*.md` that `zensical.toml`'s
  `exclude_docs` does not exclude — not `nav` — so an unlisted page is red. `inventory.toml` gains
  any page that becomes visible.
- `python/tests/test_runnable_docs.py` (`unittest`, stdlib only) covers: fence shapes, the five
  scan failures, orphan-page detection, `reason` floor, `report_result` counts, and a
  `Fence.commands()` heredoc case. It must fail on the pre-fix `fences.py` for at least the
  indented/tilde/four-backtick/orphan rows.
- `make docs-test` stays green on all 21 pages / 38 fences, and `RESULT.md` pastes the tails.

# Notes

- Do not edit `docs/**` content here; `inventory.toml` entries only.
- `python/tests/test_runnable_docs.py` is collected by the `pytest-plugin` CI job
  (`pytest python/tests`), which is how this task also closes the "gate has no tests" gap.
- Adding `tests/test_runnable_docs.py` is a new standard-tier file: not protected, no
  `critical_guard` involvement.
