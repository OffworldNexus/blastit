---
id: MB.2.6
title: Wire static routes into blastit dev and blastit check diagnostics
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.1, MB.2.5.2]
paths:
  - crates/blastit/src/cli.rs
  - crates/blastit/src/config.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/check.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/tests/critical_static_cli.rs
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-13T23:35"
---

# Goal

Make the documented local and CI flows real: a static/SPA site boots under `blastit dev` with **no `app`** when every route is static, a bad `[static]` root is reported precisely at startup, and `blastit check` treats static routes honestly (exercised vs never requested) without changing the v1 JSON contract.

# Context

- `crates/blastit/src/config.rs`: `app` becomes optional **iff** every route is static (in `dev` and production); otherwise the existing `BLASTIT_APP` error is unchanged. `docs/frameworks/static-and-spa.md` promises this.
- `crates/blastit/src/stage0.rs`: startup ordering keeps the `MB.1.6` fix — a missing `./blastit.toml` is named before `BLASTIT_APP` — and manifest validation messages for a bad static root surface verbatim with `file:line` before any process forks.
- `crates/blastit/src/cli.rs`: the `blastit dev` banner reports static routes and their roots (no new flags); `blastit check --json` keeps its exact shape.
- `crates/blastit/src/check.rs` + `audit_v1.rs`: **warnings only** — a declared `[static]` root never requested by the suite, and a static route never exercised. Do **not** add a `fix.action` and do **not** add a finding kind: `crates/blastit-py/tests/critical_check_schema.rs` and `python/tests/critical/test_check_contract.py` must stay green untouched (they are not in `paths`).
- A new protected suite `crates/blastit/tests/critical_static_cli.rs`; the existing `critical_config_and_presets.rs` and `critical_check_engine.rs` are read-only here.
- Budget: ≤ 6 files, ≤ 600 changed lines including tests.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_static_cli --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_config_and_presets --locked    # untouched, green
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_check_engine --locked          # untouched, green
$ CARGO_NET_OFFLINE=true cargo test -p blastit-py --test critical_check_schema --locked       # untouched, green
$ make critical-guard BASE=develop NODE=MB.2.6
$ make check
```

# Notes

- The critical suite proves the exit-code matrix: static-only manifest with no `app` boots under `dev`; a manifest mixing static and app routes without `app` still fails with the old message; a missing static root fails with the root path and manifest line; `blastit check` on a static route the suite never requested prints a warning and does not change `ok` or the exit code.
- If a completeness warning cannot be expressed without a new finding kind, stop and record a follow-up instead of touching the closed `fix.action` set or the Python contract.
- `docs/reference/cli.md` and `docs/troubleshooting.md` are **not** in `paths`; if the CLI text needs a doc change, log it as a follow-up and let the lead route it.

# All-static `check` path (added at the MB.2.1 gate)

`docs/frameworks/static-and-spa.md:97-99` promises the CI one-liner for a site with **no app at all**; that promise has no owner below `MB.2.6`, so this node owns it:

- `blastit check` on a manifest whose routes are all static must **not** spawn pytest: there is nothing to exercise. It audits every declared `[static]` `root` (exists, is a directory, is readable) and exits `0` when they all pass, `2` when a root is missing or not a directory — the same exit code the harness already uses for a bad manifest, and the same `--json` shape (`ok`, `findings`, `schema_version` unchanged).
- A route with no `app` in an all-static tree is `unexercised_route`-free (nothing can request it in-process); a declared root that exists but was never requested stays the warning this node already plans.
- Pinned in the same protected suite (`critical_static_cli.rs`), both directions: all roots present → exit `0`, one root missing → exit `2` naming the root and the manifest line.
- Extend the suite's exit-code matrix accordingly; do not change `fix.action`, the finding kinds, or the Python contract.

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_static_cli --locked   # includes the all-static check legs
$ ./target/debug/blastit check --json   # in an all-static fixture: exit 0, no pytest spawned, ok true
```
