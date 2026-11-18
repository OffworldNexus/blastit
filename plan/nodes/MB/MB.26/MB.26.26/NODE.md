---
id: MB.26.26
title: "Run the view-crate mutation campaigns against the tests that cover them (--test-package)"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: [Makefile, "scripts/**", "docs/testing/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T18:23"
updated: "2026-11-18T18:23"
---

# Goal

Make the per-crate mutation campaigns see the tests that cover the moved view
crates: add a `--test-package` mode to `make mutants-rust` (or a second target)
that runs `blastit`'s suites against mutated `blastit-view-php`, and
`blastit-view-python`'s suites against mutated `blastit-view`, and record the
resulting kill rate in the audit.

# Context

`MB.36.7`'s audit measured:

| crate | `cargo mutants -p <crate>` | why it understates coverage |
|---|---|---|
| `blastit-view` | 191/222 = **86.0 %** | `path.rs`, `wire.rs`, `flow.rs` survivors; their tests moved to `blastit-view-python` |
| `blastit-view-static` | 218/256 = **85.2 %** | `rewrite.rs` survivors (`declares_body`, `content_length`) are asserted by `crates/blastit-scheduler/tests/critical_rewrite.rs:509` |
| `blastit-view-php` | far below the 75 % bar | `discovery.rs` (the stage-0 gate: refusal sentences, ELF probes, candidate order) is exercised by `crates/blastit/tests/critical_php_embed.rs` and `stage0::php_boot_gate` (27 unit cases) in the **root** crate |

cargo-mutants 27.1.0 supports `--test-package` and `--cargo-test-arg`, so the
honest per-crate number is one flag away; the Makefile is the one place that
declares it.

# Acceptance

```console
$ PKG=blastit-view-php make mutants-rust-consumer
$ PKG=blastit-view make mutants-rust-consumer
```
- each prints the killed ratio computed from `mutants.out/{caught,missed,timeout,unviable}.txt`
  (never `outcomes.txt` — 27.1.0 does not write it);
- the target's help says which consumer package runs which crate's tests, and a
  crate with no consumer mapping is an error rather than a silent no-op;
- `make check` stays green and no campaign runs beside another build.

# Notes

Do not change a test to raise a ratio; a survivor that is a real gap is a node.
`MUTANTS_TMP` must stay per-checkout: two campaigns sharing it kill each other
(`LESSONS.md`).

# Provenance

Re-filed from `MB.36.9` (the `MB.36` audit) so the deferred gap stays actionable:
priority-3 coverage, not an `MB.36` acceptance claim. The body above is `MB.36.9`'s own, unchanged.
