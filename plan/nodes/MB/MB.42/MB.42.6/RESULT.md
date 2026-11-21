# `MB.42.6` — an unused store no longer tells the reader to remove a `proxy` directive

**Status: done.** Human `blastit check` now says

```
  proxy analytics: declared, 0 statements observed
      fix: remove the store, or add the call that uses it
```

and no longer instructs the reader to remove a `` `proxy <name>` `` directive the
`Blastfile` grammar does not have. Detection, exit codes (`0/1/2/3`), streams
(report → stdout, refusals/hints → stderr) and the `--json` document are
untouched.

## Decision: the human line is worded to the existing `{"action":"observe"}`, no `to` is invented

`proxy_unused` keeps `fix: {"action": "observe"}` and gains no `fix.to`. Reasons,
in the order the node's two options were weighed:

1. **No `Blastfile` line can be the `to`.** A store is declared by
   `postgres $DATABASE_URL` (unnamed, lowered to `db`) or
   `postgres analytics $ANALYTICS_URL` (named). The *node* is a store, but the
   only complete lines name it **with its `$URL` or its `{ … }` block**; the
   header fragment `postgres analytics` is not a line a reader can type, and for
   the unnamed store there is no name at all to interpolate. The node forbids
   inventing a `to` no line satisfies, so a truthful `to` would have to carry the
   protocol (`postgres`) plus the environment-variable slot — protocol and slot
   the audit report deliberately does not read back into `Fix`.
2. **`Fix::to` is documented as "the Blastfile node the action edits", and
   `observe` edits nothing** (`crates/blastit-audit/src/findings.rs:40-46`,
   `docs/reference/blastfile.md:499`). Attaching a `to` to a non-editing action
   would make the machine shape contradict its own documented meaning, and it
   would duplicate the `proxy` key that already names the same store.
3. **The documented contract already pairs these two forms.** `docs/ci.md:104`
   gives `proxy_unused` the fix "fix the suite's URL, or remove the store" while
   its `--json` example (and `docs/design/ma-lean-dx.md:323`) carries
   `{"action":"observe"}`. `observe` means *no single mechanical edit is
   proposed; the author decides*, which is exactly what the sentence now says:
   the two ways to resolve the finding, with no invented directive. So the
   rewording makes the human line carry the same "you choose" shape as `observe`
   instead of claiming a `proxy <name>` edit the file cannot express.

The noun `proxy` stays on the finding's *statement* line (`proxy analytics:
declared, 0 statements observed`) and on `on proxy <name>` in the
`missing_grant`/`dead_grant` lines: those name the runtime proxy a statement ran
on (or was denied by), not a `Blastfile` directive, and `--json` documents
`proxy` as the key for a store until the cutover re-spells it
(`docs/reference/blastfile.md:484-489`). Only the backticked pseudo-directive —
the one string that claimed grammar the file does not have — changed.

## Changes

| File | Change |
|---|---|
| `crates/blastit-audit/src/human.rs` | `Finding::ProxyUnused`'s fix line is now `fix: remove the store, or add the call that uses it`, with a comment recording that the declaration is a store and that `observe` carries no single edit. One line, no trailing period, same indent. |
| `crates/blastit/tests/critical_check_engine.rs` | The pinned text at `:906`, `:939`, `:941` (two scenarios) only. No scenario, expectation, count or other case touched. Authorized by this node's `paths` + `# Goal`; `critical_guard` confirms it. |
| `README.md` | `:93` in the `text title="a failing run — three of its six findings"` block — an illustrative `text` fence (not parsed by `docs_fences`), which pasted the old line verbatim and would otherwise show output the tool no longer prints. |

No other production code changed: same finding construction in
`crates/blastit-audit/src/evaluate.rs:259-267`, same `Fix::observe()`, same
ordering, same exit-code path.

## Grep sweep for the wrong noun

`grep -rn 'remove \`proxy|remove proxy|proxy <name>|proxy: declared|add the call that uses it'`:

| Hit | Action |
|---|---|
| `crates/blastit-audit/src/human.rs` | fixed (the bug). |
| `crates/blastit/tests/critical_check_engine.rs:906,939,941` | pinned text updated. |
| `README.md:93` | updated (rendered sample). |
| `python/tests/runnable_docs/harness.py:1469`, `python/tests/test_check.py:553` | only the `proxy_unused` **kind** name — no text to fix. |
| `docs/ci.md:104` | already correct ("remove the store"); no edit needed. |
| `plan/**/RESULT.md`, `plan/JOURNAL.md`, `plan/STATE.md`, this NODE | historical records; deliberately untouched. |

## Measured output (driven, not retyped)

Real binary `target/debug/blastit` (built from this tree) in a copy of
`python/tests/runnable_docs/fixtures/findings`, with
`python/tests/runnable_docs/pgstub.py` as the upstream (`PgStub("127.0.0.1", 0)`),
`DATABASE_URL`/`ANALYTICS_URL` pointed at it and
`BLASTIT_PYTHON=.venv/bin/python`:

```console
$ blastit check      # exit 1
....                                                                     [100%]
4 passed in 0.07s
blastit check: 4 requests, 2 statements, 3/4 views, 2 proxies
  view catalog: 1 statement not granted on proxy db
    SELECT 2
      fix: add it to view catalog postgres
  view search: 1 grant never used on proxy db
    SELECT 9
      fix: remove it from view search postgres
  view checkout: never exercised
      fix: add a test that reaches it
  view upload: takes a body and declares no ingress validation; observed application/x-www-form-urlencoded
      fix: write `body form`
  request GET /robots.txt: no view matches
      fix: add a `view` whose pattern matches this request
  proxy analytics: declared, 0 statements observed
      fix: remove the store, or add the call that uses it
blastit check: 6 problems · `--json` for the machine-readable list

$ grep -n -A1 'proxy analytics: declared' human.txt
16:  proxy analytics: declared, 0 statements observed
17-      fix: remove the store, or add the call that uses it

$ blastit check --json      # exit 1, same run
[{"kind": "proxy_unused", "severity": "error", "proxy": "analytics", "fix": {"action": "observe"}}]
```

`stderr` was empty on both runs, and both exited `1`.

## Acceptance

```console
$ cargo test -p blastit --locked --test critical_check_engine
test the_body_finding_names_the_view_body_node_and_never_unvalidated ... ok
test every_finding_kind_and_its_interaction_counts ... ok
test the_r27_golden_is_a_clean_audit_after_the_edit ... ok
test the_r27_golden_proposes_one_orders_route_with_both_exact_queries ... ok
test a_clean_run_is_one_line_and_ok ... ok

test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

```console
$ cargo test -p blastit-audit --locked
running 78 tests
test result: ok. 78 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
running 3 tests
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.6
critical-guard: HEAD..working tree, 4 changed file(s)
node MB.42.6: paths ['crates/blastit-audit/**', 'crates/blastit/tests/critical_check_engine.rs']
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.42.6
```

```console
$ make fast-check      # exit 0
PYTHONHOME="…/cpython-3.14.10-linux-x86_64-gnu" PYO3_PYTHON="…/bin/python3.14" … cargo test -p blastit --locked --features python --test critical_handoff
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Running tests/critical_handoff.rs (target/debug/build/blastit/9bb580532eaf7322/out/critical_handoff-9bb580532eaf7322)

running 15 tests
…
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

`cargo fmt --all` was run before the gate; `gate: rustfmt`, `clippy -D warnings`,
`rs-doc`, `rs-test`, `py-lint`, `guard`, `critical-guard` and `plan validate` all
passed (the tail above is the gate's last target).

## Follow-ups

- `docs/design/ma-lean-dx.md:57` still sketches the unused-store line as
  `` proxy <P>: declared, 0 statements observed `` — a dated design doc under
  `docs/**`, out of scope here; for `MB.42.4` if that page is meant to stay a
  live reference.
- `docs/reference/blastfile.md:487-489` notes that `proxy` will be re-spelled
  `store` at the cutover; when that lands, the human statement line
  `proxy <name>: declared, …` and the `--json` key should move together
  (`MB.42.4`).
