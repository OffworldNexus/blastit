# RESULT — MB.3.14 attempt 3

Appended **two** bullets to the end of `docs/knowledge/LESSONS.md`, from `MB.3.12`'s measured
findings (its LOG follow-up (a)). The three existing bullets above them are untouched.
Attempt 2's single-sentence addition to the design-doc bullet is still in place.

Note the mismatch: `NODE.md` still describes the design-doc collision lesson (already written and
committed at line 37, `dc05a48`); the dispatch directive superseded it with `MB.3.12`'s two
follow-ups. Wrote what was dispatched.

## Acceptance

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.23s

$ uv run --no-project tools/plan.py validate
plan OK
```

`make check` deliberately **not** run: a sibling node is mid-edit under `crates/**` (the tree shows
unrelated in-flight changes), so a red run would prove nothing. Nothing staged or committed.

## Bullets (as written)

> **`php_execute_script` swallows the bailout, so the obvious guard is not the one that fires:** a script's `exit()`/`die()`/fatal comes back from `php_execute_script` as a **`false` return**, not as a `longjmp` into the surrounding `zend_first_try { … } zend_catch { … } zend_end_try()` — the wrapper is still mandatory (unwinding across Rust frames is UB), but a worker that treats `true`/`false` as "script ok / script failed" and leans on the outer catch alone leaves `exit()` unhandled on the status path. Read the status for the swallowed cases, keep the guard for the paths that do jump, and drive `exit()`, a fatal and a clean run through the same entry point in one test. (`MB.3.12`).

> **`CLOCK_PROCESS_CPUTIME_ID` is tick-granular, so `cpu_ns > 0` is not a valid assertion on a fast machine:** a trivial fixture can legitimately measure **0** ns of process CPU because it never crosses a timer tick, so a probe that asserts strictly `> 0` fails intermittently on fast hosts and reads like a broken measurement rather than a fixture that is too cheap. Make the fixture burn at least one tick (a small busy loop, no `usleep` — sleeping spends wall time, not CPU), or assert `cpu_ns > 0 || wall_ns < tick_floor` against a documented floor; never conclude "the measurement works" from a single fast run. (`MB.3.12`).

## Follow-ups

- `NODE.md` title/Goal/Context for this node are stale w.r.t. attempt 3's dispatch; the lead may want
  to retitle it (or file a separate node) so the plan tree matches what was written.
