---
id: MB.3.6.1.2
title: "Workspace Cargo edges, Cargo.lock and the fastcgi-codec conformance driver - design MB.3.6a1 part 2"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.1.1]
paths:
  - Cargo.lock
  - "crates/blastit-scheduler/Cargo.toml"
  - "crates/blastit-fastcgi/Cargo.toml"
  - "crates/blastit-fastcgi/tests/conformance.rs"
  - crates/blastit/Cargo.toml
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T15:14"
---

# Goal

Land the serialized dependency/lock window and the FastCGI **codec** conformance driver:
the workspace `Cargo.toml` edges plus `Cargo.lock`, `"fastcgi-codec"` registered against
the shared table, and the `Makefile` line that puts it in `rs-test-conformance` (design
leaf `MB.3.6a1`, part 2).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.2 "crates by seam" (crate-edge matrix),
  §4.11 (`IMPLEMENTATIONS` + `implementation(kind)`), §8's `MB.3.6.1.2` row and §8.2's
  `MB.3.6.1` row.
- Files (the whole leaf, 6): `Cargo.lock`, `crates/blastit-scheduler/Cargo.toml`,
  `crates/blastit-fastcgi/{Cargo.toml,tests/conformance.rs}`, `crates/blastit/Cargo.toml`,
  `Makefile`.
- What lands:
  - `crates/blastit-scheduler/Cargo.toml`: `blastit-fastcgi` (pure) and
    `blastit-backend` workspace deps.
  - `crates/blastit-fastcgi/Cargo.toml`: `[dev-dependencies] blastit-backend` (the only
    edge that crate ever gains; production stays dependency-free).
  - `crates/blastit/Cargo.toml`: `blastit-backend = { version = "0.2.0", path = "../blastit-backend" }`
    and `blastit-fastcgi` (the composition root builds/prints `RunError`).
  - `crates/blastit-fastcgi/tests/conformance.rs`: the `"fastcgi-codec"` driver iterating
    `blastit_backend::conformance::cases()` against the codec-level implementation; no
    `case.id` literals.
  - `Makefile`: append `-p blastit-fastcgi --test conformance` to `rs-test-conformance`
    (4 binaries after this leaf).
- **`Cargo.lock` is yours and this is the fourth link of the release chain**
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`). This leaf adds no new
  workspace member, only edges: run the first build offline **without** `--locked` to
  update the lock, then `--locked` everywhere.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --test conformance
$ cargo test -p blastit --locked
$ cargo tree -p blastit -e normal | grep -q blastit-backend && cargo tree -p blastit -e normal | grep -q blastit-fastcgi && echo "composition root sees both"
$ cargo tree -p blastit-scheduler -e normal | grep -q blastit-fastcgi && echo "scheduler edge exists"
$ make rs-test-conformance
$ git diff --stat Cargo.lock && echo "lock updated in this leaf"
$ make check
```

# Notes

- If the codec driver cannot compile without the session impl, register the codec-level
  implementation only and say so in `RESULT.md` — do not move session code into this leaf.
- `Makefile` is also touched by `MB.3.6.2.2` (5-binary assertion) and `MB.9.4`
  (target creation); the three edits are append-only and serialized by `depends_on`.
- No source file of any crate is in `paths`: this leaf is manifests, lock and one test.
