---
id: MB.9.1.2
title: "Conformance case table, IMPLEMENTATIONS and cases() in blastit-backend - design MB.9.1 part 2"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.1.1]
paths: ["crates/blastit-backend/src/lib.rs", "crates/blastit-backend/src/conformance.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T10:51"
---

# Goal

Add the shared conformance case table to `blastit-backend`: the typed vocabulary
(`Case`, `Class`, `Capability`, `Expect`, `Inject`, `RouteFixture`, `HeadSpec`,
`OutcomeKind`, `PeerScript`, `CancelAt`), the exact `IMPLEMENTATIONS` slice,
`implementation(kind)` and `cases()` (design leaf `MB.9.1`, part 2). Data and lookup
only — the runner and the fake double are `MB.9.4`'s.

# Context

- Design of record: `ADR-0008`'s surviving `Upstream` contract (the table, the drivers, the
  anti-drift rule) over the contract types of `MB.9.1.1`; the retired design's §4.11 is
  deleted with `ADR-0010` and is not a source.
- Files (the whole leaf, 2): `crates/blastit-backend/src/{lib,conformance}.rs`. `lib.rs`
  appends `pub mod conformance;` (the crate root and the `pub use` list are `MB.9.1.1`'s).
- What lands:
  - `Case` (id, class, request head, expected outcome), `Class` (the four the design
    names: refusals, body caps, observation records, error mapping), `Capability`
    (`Clone, Copy, PartialEq, Eq, Debug`), `Expect`, `Inject`, `RouteFixture`, `HeadSpec`,
    `OutcomeKind`, `PeerScript`, `CancelAt`.
  - `pub const IMPLEMENTATIONS: &[(&str, &[Capability])]` — the exact slice the code
    registers (`static`, `wsgi`, `php`, `fake`); the retired external-PHP codec and session
    drivers were removed in `MB.3.17` and are not in the table. Plus
    `implementation(kind) -> Option<&'static [Capability]>`.
  - `cases() -> &'static [Case]` — the single accessor every driver iterates.
- Vocabulary is language-agnostic: no `Php*`/`Python*`/`Static*` case names, no
  `Query`-emitting case (`ObservationKind::Query` is reserved in `0.2.0`), no `case.id`
  literals in any driver.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit-backend --locked
$ grep -q 'pub fn cases' crates/blastit-backend/src/conformance.rs && grep -q 'IMPLEMENTATIONS' crates/blastit-backend/src/conformance.rs && echo "one table + one implementation registry"
$ grep -q 'pub mod conformance;' crates/blastit-backend/src/lib.rs && echo "table is reachable from the crate root"
$ make check
```

# Notes

- Keep `cases()` allocation-free (a `&'static [Case]`), so every driver can iterate it per
  test without setup.
- The table's integrity assertions (unique ids, every class ≥ 1 case, `Expect`
  class-completeness) and the fake `Upstream` double are `MB.9.4`'s — do not duplicate
  them here.
- Adding a case for PHP is `MB.3.6`'s job; this leaf only pins the shape.
