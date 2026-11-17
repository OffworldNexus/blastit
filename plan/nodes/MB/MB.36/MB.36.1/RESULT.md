# RESULT — MB.36.1 (design the view-crate graph, ADR-0016)

Attempt 2/3 (attempt 1 was swapped before producing files). Worktree
`.worktrees/release-0.2.0`, branch `release/0.2.0`.

## What was written

| Path | What |
|---|---|
| `docs/design/view-crates-architecture.md` | the design of record (~990 lines): crate graph + DAG, file-by-file module map with line/case counts, feature matrix and exact `-p` build commands with the guards, the six seam decisions with their rejected alternatives, the critical-test relocation table with the count oracle, the security table, R1–R18 with oracles, the leaf order, and every review resolution |
| `docs/decisions/ADR-0016-view-crates-over-wire.md` | one-page ADR: Context, six-item Decision, Rejected alternatives, Consequences |
| `docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md` | **status line only**: `Accepted — Superseded in part by ADR-0016 (…)`; decisions untouched (diff is one hunk) |
| `docs/decisions/README.md` | the ADR-0016 row, and ADR-0009's status cell says `Accepted (superseded in part by ADR-0016)` |

Four fresh-context critic reviews live beside the design
(`docs/design/view-crates-architecture.review-{security,implementability,testability,consistency}.md`).
The design was revised through eight rounds; §9 of the document records every finding and its
resolution, including the findings that changed the design materially (probe `setpgid` inside
`enter`, the opaque entry pin, `Privilege`/`Filter` types, the `%25` `debug_assert` removal, the
`MB.36.3` feature-drop ordering, the `static` feature's scheduler half, and the plan-node
`paths`/acceptance defects recorded as P1–P5).

## Evidence

```console
$ test -f docs/design/view-crates-architecture.md && test -f docs/decisions/ADR-0016-view-crates-over-wire.md && echo "design and ADR exist"
design and ADR exist
$ for l in security implementability testability consistency; do test -f docs/design/view-crates-architecture.review-$l.md && echo "$l review exists"; done
security review exists
implementability review exists
testability review exists
consistency review exists
$ grep -c 'Superseded in part by ADR-0016' docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md
1
$ git diff --stat docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md
 docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md | 2 +-
 1 file changed, 1 insertion(+), 1 deletion(-)
$ grep -q ADR-0016 docs/decisions/README.md && echo "ADR indexed"
ADR indexed
$ grep -cE 'blastit-view-python|blastit-view-php|blastit-view-static' docs/design/view-crates-architecture.md
96
$ uv run --no-project tools/plan.py validate
plan OK
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py scripts/test_doc_tone.py -q
48 passed in 1.32s
$ uv run --no-project tools/guard.py
guard: OK (61 staged/untracked file(s) checked)
$ wc -l docs/design/view-crates-architecture.md docs/decisions/ADR-0016-view-crates-over-wire.md
  1066 docs/design/view-crates-architecture.md
   140 docs/decisions/ADR-0016-view-crates-over-wire.md
```

## Acceptance

- [x] `docs/design/view-crates-architecture.md` and `ADR-0016` exist.
- [x] ADR-0009's header contains `Superseded in part by ADR-0016`; its decisions are untouched.
- [x] ADR-0016 is indexed in `docs/decisions/README.md`.
- [x] The design names `blastit-view-python`, `blastit-view-php` and `blastit-view-static`, and
      the relocation table carries `critical_static_resolve`.
- [x] The design contains the crate graph, the file-by-file module map with line counts, the
      feature matrix, the six seam decisions with rejected alternatives, and the relocation table.
- [x] Four reviews exist and each ends `VERDICT: PASS` (table above; zero BLOCKER, zero MAJOR).
- [x] No **`docs/`** path outside this node's four changed by this node: `git status --porcelain -- docs/`
      lists exactly `ADR-0009` (one status line), `decisions/README.md` (two rows),
      `ADR-0016`, `view-crates-architecture.md` and its four review files.
      (`docs/design/dev-fanotify-reload.md` also shows modified — that is another lead's
      in-flight `MB.23.5.2` edit in this shared worktree, not this node's.)

## Review verdicts

| Lens | File | Verdict (last line of the file) |
|---|---|---|
| security | `docs/design/view-crates-architecture.review-security.md` | `VERDICT: PASS` (0 BLOCKER, 0 MAJOR) |
| implementability | `docs/design/view-crates-architecture.review-implementability.md` | `VERDICT: PASS` (0 BLOCKER, 0 MAJOR) |
| testability | `docs/design/view-crates-architecture.review-testability.md` | `VERDICT: PASS` (0 BLOCKER, 0 MAJOR) |
| consistency | `docs/design/view-crates-architecture.review-consistency.md` | `VERDICT: PASS` (0 BLOCKER, 0 MAJOR) |

```console
$ for l in security implementability testability consistency; do printf '%-18s %s\n' "$l" "$(tail -1 docs/design/view-crates-architecture.review-$l.md)"; done
security           VERDICT: PASS
implementability   VERDICT: PASS
testability        VERDICT: PASS
consistency        VERDICT: PASS
```

The design went through eight review rounds (four lenses, fresh context each time); §9 of the
document records every finding, its resolution, and the rounds in which a finding was re-raised
and re-fixed. The accepted non-gating notes from the final passes are recorded there too (e.g.
`for_test`'s spelling, `build-standalone.sh --require-no-python`, the `critical_dev_reload`
count being a grow-only floor).

## Follow-ups (out of scope for MB.36.1)

1. **Plan-node path amendments (P1/P2).** `MB.36.2`, `.3`, `.4`, `.5` and `.6` each need a few
   globs before dispatch; §8.0 lists every one with its reason (`plan.py edit --paths` replaces
   the list, so paste the whole list).
2. **Plan-node acceptance lines (P4/P5).** `MB.36.3`–`.5` and `.7`'s `critical-guard
   BASE=develop` lines, `MB.36.6`'s `WorkerRuntime` grep, `MB.36.5`'s `libphp8` grep and
   `cargo tree` line, `MB.36.3`'s dead `critical_loaded_files` target and its body's tracker
   sentence, and `MB.36.7`'s missing capture/`dx_invariance`/`make check` lines — all recorded in
   §8.0/§8.5 of the design; the lead owns the edit.
3. **The epic's own guard line is vacuous** (`MB.36`'s `paths` is `crates/**`); the per-leaf runs
   plus the release-wide `--node` list are the real gate.
4. **`docs/design/backend-runtimes.md` §4.1–§4.2/§4.7** still describe the deleted trait and the
   old archive command; P2 assigns the header line and the §4.7 correction to `MB.36.6`.
5. **`MB.36.5` is a ~5 000-line leaf**; §8.6 records the optional a/b split (worker +
   `runtime.rs` deletion, then discovery/pinning/probe).
6. **`critical_dev_reload.rs` is an untracked, in-flight file** (owned by `MB.23.5.2`); its
   14-case/1 619-line count is re-measured at the leaf and may only grow.
