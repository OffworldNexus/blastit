# Result MB.1.10 — `add_group` documented, MB.1.6 gotchas recorded

**STATUS: done** — `docs/ci.md` names every fix action the binary can emit and shows the group shape; the two `MB.1.6` gotchas and both `cargo-mutants` gotchas are in `LESSONS.md`; `make docs-check` and `make docs-unit` are green. No Rust, no behaviour change, no new public page.

## Summary

- **`docs/ci.md`**: `fix.action` now lists `add_group` with the other eight, plus two prose lines and a four-line TOML block the page did not have: what `add_group` is for (session setup — `SET`, `SHOW`, `set_config(…)`), why (`it runs on whichever route connects first`), and the exact manifest shape (`groups = ["connection"]` + `[group.connection.db]` with the statement). The `missing_grant` row of the "What it fails on" table now routes session setup to the `connection` group, so the one-canonical-fix-per-finding table no longer points a `set_config` denial at a single route's `queries`. Growth: **150 → 158 lines (+8, the budget)**, 12 insertions / 4 deletions.
- **`docs/design/ma-lean-dx.md`**: `R33` now states the `add_group` branch and that the emitted `add_group` also passes `Manifest::parse_and_validate`; `R38`'s closed `fix.action` set gains `add_group`; the §4.2 worked contract gains the connection-setup `missing_grant` document; the §7.1 oracle line now reads "every `add_query`/`add_group` … re-parses cleanly".
- **`docs/knowledge/LESSONS.md`**: four new bullets — the three the node specified (allocation oracle, stage-0 `app.output` stream, `cargo-mutants` outcome files) plus one it did not: two concurrent `cargo mutants` runs must not share `MUTANTS_TMP` — and the stale `add_query`-on-one-route sentence in the Django/`TimeZone` bullet corrected to the shipped `add_group` behaviour; that bullet also now links `reference/manifest.md` §Groups instead of a path that does not exist.

**Snippet note (acceptance 1):** the node's own regex (`"([a-z_]+)"\s*=>|action:\s*"([a-z_]+)"`) returns the **empty set** against `audit_v1.rs`, because actions are built as `Self::new("add_group")`, so that snippet asserts nothing. Widened to `Self::new\("([a-z_]+)"\)` (the only constructor that sets `action`) as the node's Notes allow; it reads 8 emitted actions and confirms the page names every one. Both runs are pasted below.

**Non-actions:** no fixture, no `python/tests/critical/test_check_contract.py`, no action renamed, nothing outside the three `paths` touched (`git status` shows only my three docs files plus work that is not mine). `make check` was **not** run: a `cargo-mutants` campaign owns this working tree.

## Evidence

Environment: `release/0.2.0` working tree, host uid 1000, offline uv (Python 3.14.10).

### 1. `docs/ci.md` names every emitted fix action

```console
$ grep -n 'add_group' docs/ci.md
74:the manifest: `"[route.db].queries"`. `fix.action` is `add_query`, `add_group`,
76:`fix_ingress` or `observe`; a destructive one carries `"unsafe": true`. `add_group`
```

The node's snippet as written, unchanged:

```console
$ uv run --no-project python - <<'PY'
import pathlib, re
page = pathlib.Path("docs/ci.md").read_text()
src = pathlib.Path("crates/blastit/src/audit_v1.rs").read_text()
actions = {m for m in re.findall(r'"([a-z_]+)"\s*=>|action:\s*"([a-z_]+)"', src) for m in m if m}
print(sorted(actions))
missing = [a for a in actions if f"`{a}`" not in page]
assert not missing, missing
print("docs/ci.md names every emitted fix action")
PY
[]
docs/ci.md names every emitted fix action
```

`[]` is the whole problem: the assertion is vacuous. Widened to the constructor that actually sets `action`:

```console
$ uv run --no-project python - <<'PY'
import pathlib, re
page = pathlib.Path("docs/ci.md").read_text()
src = pathlib.Path("crates/blastit/src/audit_v1.rs").read_text()
# `Fix::new(...)` is the only constructor that sets `action`; every variant goes through it.
actions = sorted(set(re.findall(r'Self::new\("([a-z_]+)"\)', src)))
print("emitted:", actions)
missing = [a for a in actions if f"`{a}`" not in page]
assert not missing, missing
print("docs/ci.md names every emitted fix action")
PY
emitted: ['add_group', 'add_query', 'add_route', 'add_test', 'declare_ingress', 'fix_ingress', 'observe', 'remove_query']
docs/ci.md names every emitted fix action
```

`remove_route` is documented but currently unemitted (reserved in `R38`), so the page is a superset of the source — the contract holds in the direction that matters.

### 2. The group sentence shows the shape and the reason

```console
$ grep -n '\[group\.' docs/ci.md
82:[group.connection.db]
```

```console
$ git diff docs/ci.md | sed -n '/add_group`$/,/^```$/p' | tail -14
+`fix_ingress` or `observe`; a destructive one carries `"unsafe": true`. `add_group`
+is for session setup (`SET`, `SHOW`, `set_config(…)`): it runs on whichever route
+connects first, so `fix.to` names a group every route attaches:
+
+```toml
+groups = ["connection"]                 # to every route
+[group.connection.db]
+queries = ["SELECT set_config('TimeZone', $?, false)"]
+```
 
 `--json` writes one document to stdout and nothing else: pytest output, logs and
 `learn:` lines go to stderr, exit `2` leaves stdout empty, and under `--learn` it
```

The fence is valid TOML and is the shape the manifest parser accepts (top-level `groups` above the first table, `[group.<name>.<proxy>]` grants):

```console
$ uv run --no-project python - <<'PY'
import re, pathlib, tomllib
page = pathlib.Path("docs/ci.md").read_text()
for fence in re.findall(r"```toml\n(.*?)```", page, re.S):
    print("parses:", tomllib.loads(fence) is not None)
PY
parses: True
```

### 3. Leanness: +8 lines, exactly the budget

```console
$ uv run --no-project python -c 'import pathlib; print(len(pathlib.Path("docs/ci.md").read_text().splitlines()))'
158
$ git diff --stat docs/ci.md
 docs/ci.md | 16 ++++++++++++----
 1 file changed, 12 insertions(+), 4 deletions(-)
```

150 → 158, i.e. **+8** (must not grow by more than 8). Three lines were saved on the way there: the group explanation was folded into the enumeration paragraph instead of opening a new one, and the blank line inside the fence was dropped.

### 4. The gotchas are where the next agent looks

```console
$ grep -c 'allocation oracle\|second pass' docs/knowledge/LESSONS.md
1
$ grep -c 'stage-0\|app.output' docs/knowledge/LESSONS.md
2
```

```console
$ tail -4 docs/knowledge/LESSONS.md | cut -c1-160
- **An allocation oracle must compare *second* passes in separate worlds, with a per-thread armed flag:** the first measured 100-request pass in a fresh `World`
- **A production stage-0 error is an `app.output` event on *stdout*, not stderr:** `stage0::run` surfaces startup output as `{"event":"app.output","stream":"std
- **`cargo-mutants` 27.1.0 never writes `outcomes.txt`:** it writes `mutants.out/{caught,missed,timeout,unviable}.txt` plus `mutants.json` and `outcomes.json` (
- **Two concurrent `cargo mutants` runs must not share `MUTANTS_TMP`:** `cargo-mutants` 27.1.0 scratch-copies the workspace into `$TMPDIR/cargo-mutants-<crate>-
```

Four bullets, not three: the node specified the first three, and this tree also paid for the `MUTANTS_TMP` scratch-dir collision (`MB.1.2`) — two leaves running `cargo mutants` in one `MUTANTS_TMP` delete each other's copy on start.

Each opens with the symptom and gives the fix; the `cargo-mutants` bullet is backed by this working tree's own runs:

```console
$ cargo mutants --version
cargo-mutants 27.1.0
$ ls /tmp/opencode/mb1-manifest-edit/mutants.out/
caught.txt  debug.log  diff  lock.json  log  missed.txt  mutants.json  outcomes.json  timeout.txt  unviable.txt
```

No `outcomes.txt` exists, which is the gotcha.

### 5. The design note agrees with the shipped schema

```console
$ grep -n 'add_query' docs/design/ma-lean-dx.md | head -3
131:- **R33 (`missing_grant` finding & bootable `fix` guarantee):** … `"fix"` is `{"action": "add_query", "to": "[route.<proxy>].queries"}` — except for connection setup (`KNOWN_STATIC_SETUP_SQL`, or a single `SET`/`SHOW` passing `is_introspection_setup_sql`), which emits `{"action": "add_group", "to": "[group.connection.<proxy>].queries"}` …
136:- **R38 (`blastit check --json` count & schema invariants):** … `fix.action` is restricted to `{add_query, add_group, remove_query, add_route, remove_route, add_test, declare_ingress, fix_ingress, observe}` …
314:   - `missing_grant` *(parameterized)*: `{"kind":"missing_grant","severity":"error","route":"catalog","proxy":"db","sql":"SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?","fix":{"action":"add_query","to":"[route.db].queries"}}`.
```

Every enumerated action list (`R33`, `R38`, §4.2's worked contract) now includes `add_group`; the only remaining bare `add_query` mentions are the per-route case (correct) and historical scope/audit rows.

### 6. Green docs gates

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.23s
```

```console
$ make docs-unit
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py -q
..............................................                                          [100%]
46 passed, 57 subtests passed in 0.09s
```

`docs-unit` includes `RepositoryInventoryTest`, so the new `toml` fence in `docs/ci.md` is accounted for by the runnable-docs gate: indices are counted over `console` fences only, and `docs/ci.md`'s five console fences keep their 1–5 numbering.

### 7. `make check` — run by lead

Not run here: a long `cargo-mutants` campaign is measuring this working tree, and any cargo invocation would corrupt it. The lead runs `make check` before committing; nothing in this node changes Rust, and neither Rust nor Python test files were touched.

### 8. Post-review fixups (accuracy pass)

Three corrections after review, all inside this node's two edited files. `docs/ci.md` gained no line, and the gates were re-run:

- **`missing_grant` table row**: session setup now reads `SET`, `SHOW`, `set_config(…)`. A Django `TimeZone` denial *is* a `set_config`, and the row named only the other two.
- **`add_group` prose**: narrowed to the shipped predicate. `is_connection_setup` (`crates/blastit/src/audit_v1.rs:783`) accepts an exact `KNOWN_STATIC_SETUP_SQL` match (`SET TIME ZONE 'UTC'`, `SELECT set_config('TimeZone', $?, false)`) or a single `SET`/`SHOW`. Any other `set_config(…)` never gets `add_query`: `check_learnable` refuses it (an inline literal, or a `$?` the parser will not take) and the finding is `observe`. Still three lines, so the page is still 158.
- **This file**: four `LESSONS.md` bullets, not three; §4's `tail -3` hid the allocation-oracle bullet and is now the real `tail -4`.

```console
$ wc -l docs/ci.md
158 docs/ci.md
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.46s
```

```console
$ make docs-unit
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py -q
..............................................                                          [100%]
46 passed, 57 subtests passed in 0.14s
```

## Files

- `docs/ci.md` — `add_group` added to the `fix.action` enumeration, `fix.to` group target, one 4-line session-setup explanation + 4-line TOML block, `missing_grant` table row now sends session setup to the `connection` group (+8 lines).
- `docs/design/ma-lean-dx.md` — `R33` group branch, `R38` closed set, §4.2 connection-setup `missing_grant` document, §7.1 oracle wording (7 insertions / 3 deletions, one line each).
- `docs/knowledge/LESSONS.md` — four new gotcha bullets (the node's three plus the concurrent-`MUTANTS_TMP` one); the Django `TimeZone` bullet's stale "known DX gap" sentence now describes `add_group`; `manifest.md` → `reference/manifest.md` in its link text.

## Follow-ups

- **`docs/ci.md:71` still says "`severity` is `error` on every kind in 0.1.1"** while the tree is on `release/0.2.0`. Verifying which kinds gained `warning` needs the binary or `audit_v1.rs` reasoning that this node's `paths` exclude, and a wrong version number is worse than a stale one. Owner: whoever next owns `docs/**` with Rust in reach.
- **`docs/ci.md`'s `--json` example does not show an `add_group` finding**, only the prose and fence do. The page is at its line budget (item 3), so adding a seventh finding line needs a matching removal; a `--json` example with `fix.to` for the group case would make the contract copy-pasteable.
- **`docs/troubleshooting.md:33` and `docs/reference/manifest.md` §Groups** already describe the group grant but do not name `fix.action: add_group` or `[group.connection.*]` as the action's target — a reader who starts from troubleshooting cannot connect the row to the `--json` field name. Both files are outside this node's `paths`.
- **`plan/nodes/MB/MB.1/MB.1.1/NODE.md:48,61` still instruct `awk '$NF=="caught"' mutants.out/outcomes.txt`**, and its Notes describe an `outcomes.txt` layout that 27.1.0 never writes — the third `LESSONS.md` bullet says so, but the acceptance block itself will mislead the next leaf that runs it. Node MB.1.1 is terminal; the correction belongs to whoever repairs plan acceptance blocks.
