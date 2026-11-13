# RESULT MA.3.5 — audit runnable-docs guard, critical tiers, scoped mutants

**Verdict: BLOCKED** (audit complete; four gaps filed as MA.3.8..MA.3.11). Report:
`docs/testing/AUDIT-MA.3.md`.

## What was done

- 15 adversarial probes against the real harness code (`check_inventory`, `public_pages`,
  `console_fences`, `Harness.expect`, `mode_check`) on `/tmp` copies. Nine real escapes, five of
  them MAJOR: fence shapes the parser never recognises (`~~~`, four backticks, indented, info-string,
  `bash` block with a prompt), a published page missing from `nav`, a `console` fence with no `$ `
  prompts passing an executing mode, and `static` deploy fences accepting nonexistent commands.
- Tier map: `docs-test`, `critical_guard` and `critical-skip-check` run in **no** CI job; the Rust
  and Python critical tests do run.
- `cargo-mutants` absent (`error: no such command: mutants`). Anchored fallback: **19/20 killed** —
  `blastit-manifest` 8/8 (all by protected suites), `blastit-sqlpolicy` 10/11 but **0 by the
  protected suite**, 1 real survivor (`MAX_INTROSPECTION_DEPTH`).

## Evidence

```console
$ make mutants-rust PKG=blastit-manifest
error: no such command: `mutants`
make: *** [Makefile:127: mutants-rust] Error 101

$ grep -rn "make " .github/workflows/*.yml
(no output)

$ uv run --no-project python /tmp/opencode/ma3_real.py
(a) unaccounted fence: [… 'docs/troubleshooting.md: 1 console fences but the inventory names []']

$ env -i HOME=/tmp/opencode/ma3-home RUSTUP_HOME=… CARGO_HOME=… make docs-test
Downloading maturin (10.3MiB)
harness: pytest==9.2.0 is not in the uv cache, and this gate never reaches the network. …
make: *** [Makefile:117: docs-test] Error 1

$ make critical-skip-check
protected critical tests hold no skip construct

$ uv run --no-project tools/critical_guard.py --base dba348c
critical-guard: dba348c..working tree, 5 changed file(s)
  no change to a protected critical test

$ make docs-test        # baseline before the scrubbed runs, 38 fences
  (MA.3.1/MA.3.2 evidence; unchanged by this audit — no test or product file was edited)
```

## Files

- `docs/testing/AUDIT-MA.3.md` (written).
- No test, product or documentation page edited; `/tmp` copies only.
