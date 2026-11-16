---
id: MB.21.4.5.5
title: "Translate the scheduler's manifest fixtures and delete its second reader"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.9]
paths:
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T16:58"
updated: "2026-11-16T16:58"
---

# Goal

The scheduler's test world boots from `Blastfile` only. The `Source::{Toml, Blastfile}` switch in
`tests/common/mod.rs` is deleted with the `Options::manifest(&str)` setter that fed it, `Options::new()`'s
default manifest is a `Blastfile`, and every suite's fixture is rewritten — the three protected suites
included, with every assertion kept.

There is one reader in the scheduler's tests, exactly as there is one reader in the product.

# Context

- Design §15.2: a second reader or a `--format` flag is a thing the system must not contain. `Source` **is**
  that second reader in test form, so it goes; `World::start` parses with `Manifest::from_blastfile_str`
  and nothing else.
- Measured at planning: 45 retired documents across 14 files — `tests/routing.rs` 11,
  `tests/critical_rewrite.rs` 9, `tests/lifecycle.rs` 5, `tests/critical_ingress_enforcement.rs` 5,
  `tests/memory.rs` 3, `tests/head_allocs.rs` 2, `tests/isolation.rs` 1, `tests/flow.rs` 1,
  `tests/fairness.rs` 1, `tests/critical_charge.rs` 1, `tests/common/mod.rs` 1 (the default),
  `src/route.rs` 3 (unit tests), `src/lib.rs` 1 (the crate's doc example), `src/config.rs` 1 (a doc
  sentence).
- `tests/critical_rewrite.rs`, `tests/critical_ingress_enforcement.rs` and `tests/critical_charge.rs` are
  protected. Every assertion stays; only fixture text and the parse call change.
- **Serialized behind `MB.21.4.9`**: it is rewriting `critical_ingress_enforcement.rs`'s head-refusal
  fixtures in the same files right now. `depends_on` holds this leaf until that node is `done`; rebase on
  its commit before editing, do not merge round it.
- Translation is mechanical and must not silently drop a construct: `prefork = true` → `prefork`,
  `egress = true` → `egress any` (warns at boot), `body = "raw"` + `allow_raw = true` →
  `body raw unvalidated`, `groups = [...]` → `use <trait>`, `[[route]] path = [...]` → one `view` with the
  same patterns (a path list stays one view with several patterns, which is what
  `routes_follow_manifest_order_and_the_uid_layout` proves).
- `src/config.rs`'s sentence names the retired file as the anchor for static roots: say "the manifest's
  directory" instead. `src/lib.rs`'s doc example is a crate-level doctest run by `make rs-doc`/`rs-test`:
  rewrite it as a `Blastfile` and keep its assertions.

# Acceptance

```console
$ grep -rnE 'from_toml_str|Source::Toml|toml::' crates/blastit-scheduler
$ grep -rnE 'blastit[.]toml' crates/blastit-scheduler
$ cargo test -p blastit-scheduler
$ make rs-doc
$ make critical-guard BASE=<sha before this node> NODE=MB.21.4.5.5
```

1. Both greps print nothing over the whole crate.
2. `cargo test -p blastit-scheduler` exits `0`; `make rs-doc` is clean (the crate doc example is a
   `Blastfile`); `critical-guard` is clean.
3. The three protected suites keep every case — paste the case count before and after.
4. `RESULT.md` carries `git diff --stat` and one line per suite naming what its fixtures still declare
   (methods, `prefork`, `body raw unvalidated`, `egress any`, several patterns in one view). A suite whose
   fixture lost a construct must name the assertion that construct carried.

# Notes

- A suite that asserts "no warning is emitted" keeps asserting it: `egress any` warns at boot, so a fixture
  that carries it either expects that warning or does not carry it.
- No new dependency and no shared test-helper crate: the world in `tests/common/mod.rs` is already the
  helper.
- Do not touch `crates/blastit-manifest/**` (other leaves) or `crates/blastit-scheduler/src/scheduler.rs`
  unless a type it names disappears — it does not: this leaf only changes how fixtures are parsed.
