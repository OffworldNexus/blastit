---
id: MB.42.15
title: "critical_artifacts asserts the deleted wheel extension (_blastit*.so); re-target at the py3-none _bin ELFs"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.4]
paths: [crates/blastit/tests/critical_artifacts.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T06:02"
updated: "2026-11-20T06:17"
---

# Goal
`critical_artifacts::the_wheel_extension_links_no_interpreter_and_no_php` asserts a subject ADR-0017 deleted. It looks for `blastit/_blastit*.so` inside the built wheel and errors:

```
Error: "…/dist/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl carries no blastit/_blastit*.so: there is no extension to check"
```

The extension is gone (`MB.42.2.2.2` retired `crates/blastit-py`); a `py3-none` wheel now carries `blastit/_bin/{blastit, blastit-lsp, blastit-dbproxy, libblastit_py3*, libblastit_php*}` instead. The test is therefore **order-dependent**: green when `dist/` is empty, red whenever a real wheel is present — which is exactly the state the release gate is in, so `make check` cannot pass.

**Authorized protected-test change (lead, 2026-11-20):** `crates/blastit/tests/critical_artifacts.rs` may be updated in this node — this node's `paths` and this `# Goal` name it.

# Exact Deliverables
1. Re-target the leg at the artifact the wheel actually ships: enumerate `blastit/_bin/*` inside the wheel and assert `assert_portable` on each **ELF** (zero `libpython`, zero `libphp`, zero `RUNPATH`) — the same invariant the extension leg existed to prove, now applied to the universal binary and the shims.
2. Keep the *shape* claim that is still true and valuable: the wheel must be `py3-none-*` (not `cp3*`), must carry **no** `_blastit*.so`, and must carry at least the binary plus the shims its platform declares. A `cp3NN` tag or a stray extension must fail with a sentence naming it.
3. Keep every other leg in the file untouched, and keep the `REQUIRE_WHEEL` behaviour (skip-with-one-line when `dist/` holds no wheel; fail when the switch is set).
4. Rename the test to say what it now proves (e.g. `the_universal_wheel_ships_no_interpreter_and_no_php`), and keep it in the same file so the guard's protection follows it.
5. Make it **not** depend on `dist/` being empty or stale: it should select the wheel for this workspace version (as now) and, if `dist/` holds an unexpected extra wheel, say so rather than silently picking one.
6. A negative: with a `cp3NN`-tagged or extension-carrying wheel the leg fails (prove it with a synthetic fixture if you cannot build one).

# Notes
- Do not delete the leg, `#[ignore]` it, or weaken the portability assertions — they are the wheel half of the R1–R5 invariant.
- Do not make the suite pass by requiring `dist/` to be empty: the release gate builds a wheel on purpose.
- Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `cargo test -p blastit --locked --test critical_artifacts` green **with** the `py3-none` wheel present in `dist/`, and green with `dist/` empty (one-line skip)
- the `REQUIRE_WHEEL=1` path still fails when no wheel is built
- a synthetic `cp3NN` / extension-carrying wheel is refused (paste it)
- `make check` completes (report the next failure by name if any)
- `make fast-check`
