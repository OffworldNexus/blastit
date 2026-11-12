# Result MA.2.1.1 — Leaf 1: workspace seams, `blastit-ingress` stub and Cargo wiring

## Summary

Attempt 1 left the `blastit-ingress` skeleton, the crate `Cargo.toml` edges and an unfinished
`blastit-env` edit on disk, with no `Cargo.lock` update and no shims. Attempt 2 finished the leaf:

- `crates/blastit-ingress/{Cargo.toml,src/lib.rs}` kept as written (constants `MAX_JSON_DEPTH`,
  `MAX_MIME_REPORT_BYTES`, `MAX_CHECK_SAMPLE_BYTES`; `BodyCodec` serde-lowercase;
  `IngressPolicy`; `#![forbid(unsafe_code)]`; `#![deny(missing_docs)]`; doctest).
- §4.1 dependency matrix verified: `blastit-ingress` (`percent-encoding 2.3.2`, `serde
  1.0.229`/derive, `serde_json 1.0.154`, `thiserror 2.0.21`, `url 2.5.8`), `blastit-manifest`
  (+`blastit-ingress`, +`serde`), `blastit-scheduler` (+`blastit-ingress`, +`serde_json`),
  `blastit` (+`blastit-ingress`, +`blastit-sqlpolicy`, +`serde`), `blastit-py` (+`blastit-ingress`).
  Root `Cargo.toml` needed no edit (`members = ["crates/*"]`).
- `Cargo.lock` regenerated offline; the only delta is the new `blastit-ingress` package plus the
  five dependency-list edges (no external version moved).
- Shims: `blastit_env::build_with_socket_dir(manifest, platform_env, strip, socket_dir)` (real
  wrapper over `build`, which now renders through a private `build_in`; `template_fields_in` is
  `pub(crate)` so the public surface stays exactly the designed one),
  `ListenerSpec::enforcing(...)`, `DbproxyConfig::enforcing(proxies, listeners)`,
  `RouteInfo::with_ingress(..)` + `RouteInfo::ingress_policy()` (fail-closed `BodyCodec::None`),
  and `..SchedulerConfig::default()` in `stage1::scheduler_config`.
- `stage0` now builds its dbproxy handover through the two `enforcing` constructors, so Leaf 7 can
  add `ListenerMode`/`route_policies` without touching init.
- No codec behaviour, no critical tests (none exist yet), no `docs/**`/`plan/` edits.

## Evidence

### 1. Lock regeneration (offline)

```console
$ CARGO_NET_OFFLINE=true cargo metadata --format-version 1 >/dev/null
# updates Cargo.lock from the local cache; only the ingress package/edges are added
$ git diff --stat Cargo.lock
 Cargo.lock | 19 +++++++++++++++++++
```

The literal acceptance spelling cannot run on this toolchain (see Follow-ups 1):

```console
$ CARGO_NET_OFFLINE=1 cargo metadata --locked --format-version 1 >/dev/null
error: error in environment variable `CARGO_NET_OFFLINE`: provided string was not `true` or `false`
exit=101
```

Equivalent, offline, and platform-filtered (exit 0):

```console
$ CARGO_NET_OFFLINE=true cargo metadata --locked --filter-platform x86_64-unknown-linux-gnu --format-version 1 >/dev/null
metadata_exit=0
```

`--filter-platform` is required because an unfiltered `cargo metadata` also downloads
Windows-only crates; `anstyle-wincon v3.0.11` (already in HEAD's `Cargo.lock`) is not in the local
cache (Follow-ups 2).

### 2. `cargo build --workspace --all-targets --locked`

```console
$ CARGO_NET_OFFLINE=true cargo build --workspace --all-targets --locked
   Compiling pyo3 v0.30.1
   Compiling blastit v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit)
   Compiling blastit-view v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-view)
   Compiling blastit-py v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.92s
```

### 3. `cargo test --workspace --all-targets --locked`

```console
$ CARGO_NET_OFFLINE=true cargo test --workspace --all-targets --locked
exit=0
$ grep -c "^test result: ok" ...
44
$ grep -E "^test result: FAILED|^error" ...
(no output)
...
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.04s
```

New coverage in this leaf: `blastit-env` `a_caller_chosen_socket_dir_reaches_the_rendered_url_only`
(default `host=/proc/self/cwd` vs `host=/run/blastit/pg`, secret stays in dbproxy's half),
`blastit-scheduler` `a_route_without_ingress_takes_no_body_and_with_ingress_keeps_its_codec`
(fail-closed default, declared `json`/`raw` survive, manifest path stays `None`), `blastit`
`the_ingress_types_are_importable_and_speak_the_manifest_spelling` (`"json"`/`"raw"` wire spelling).

### 4. `make check`

```console
$ CARGO_NET_OFFLINE=true make check
exit=0
uv run --no-project tools/guard.py
uv run --no-project tools/critical_guard.py --base HEAD
cargo fmt --all -- --check
no Rust critical tests yet (crates/*/tests/critical_*.rs)
no Python critical tests yet (python/tests/critical/)
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
No issues found
Build finished in 0.18s
uv run --no-project tools/plan.py validate
plan OK
```

(`rs-lint` clippy `-D warnings` and `rs-doc` `RUSTDOCFLAGS=-D warnings` ran green inside that
target; `cargo fmt --all -- --check` re-run separately: `fmt_exit=0`.)

### 5. Graph is acyclic (`sqlpolicy` + `ingress` -> `manifest`)

```console
$ CARGO_NET_OFFLINE=true cargo tree -p blastit-manifest --locked -e normal --depth 1
blastit-manifest v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-manifest)
├── blastit-ingress v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-ingress)
├── blastit-sqlpolicy v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-sqlpolicy)
├── regex v1.13.1
├── serde v1.0.229
├── thiserror v2.0.21
└── toml v1.1.8+spec-1.1.0

$ CARGO_NET_OFFLINE=true cargo tree -p blastit-manifest --locked --invert
blastit-manifest v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-manifest)
```

No edge back to `blastit-manifest` (and `blastit-ingress` has no workspace dependency at all).

### 6. Import compile checks

```console
$ grep -n "use blastit_ingress" crates/blastit-scheduler/src/route.rs crates/blastit/src/stage1.rs
crates/blastit-scheduler/src/route.rs:10:use blastit_ingress::{BodyCodec, IngressPolicy};
crates/blastit/src/stage1.rs:1102:        use blastit_ingress::{BodyCodec, IngressPolicy};

$ CARGO_NET_OFFLINE=true cargo tree -p blastit-scheduler -i blastit-ingress --locked --depth 1
blastit-ingress v0.1.0 (...)
├── blastit-manifest v0.1.0 (...)
└── blastit-scheduler v0.1.0 (...)

$ CARGO_NET_OFFLINE=true cargo tree -p blastit -i blastit-ingress --locked --depth 1
blastit-ingress v0.1.0 (...)
├── blastit v0.1.0 (...)
├── blastit-manifest v0.1.0 (...)
└── blastit-scheduler v0.1.0 (...)

$ CARGO_NET_OFFLINE=true cargo tree -p blastit-py -i blastit-ingress --locked --depth 1
blastit-ingress v0.1.0 (...)
├── blastit v0.1.0 (...)
├── blastit-manifest v0.1.0 (...)
├── blastit-py v0.1.0 (...)
└── blastit-scheduler v0.1.0 (...)
```

`blastit-scheduler` and `blastit` carry a compile-time `use`; for `blastit-py` the direct
dependency edge is what makes the types importable (its `src/**` is outside this leaf's `paths`,
see Follow-ups 3). `cargo build -p blastit-py` compiles `blastit-py` with
`--extern blastit_ingress=...`, which is the proof, not a `Cargo.toml`-only claim.

## Files

- `crates/blastit-ingress/Cargo.toml` (new)
- `crates/blastit-ingress/src/lib.rs` (new)
- `Cargo.lock` (ingress package + 5 edges)
- `crates/blastit-manifest/Cargo.toml`, `crates/blastit-scheduler/Cargo.toml`,
  `crates/blastit/Cargo.toml`, `crates/blastit-py/Cargo.toml`
- `crates/blastit-env/src/lib.rs`, `crates/blastit-env/src/env.rs`,
  `crates/blastit-env/src/postgresql.rs`
- `crates/blastit-scheduler/src/route.rs`
- `crates/blastit-dbproxy/src/config.rs`
- `crates/blastit/src/stage0.rs`, `crates/blastit/src/stage1.rs`
- `plan/nodes/MA/MA.2/MA.2.1/MA.2.1.1/RESULT.md` (this file)

Root `Cargo.toml` unchanged (`--locked` works with `members = ["crates/*"]`).

## Follow-ups

1. **Acceptance command is unrunnable as written.** `CARGO_NET_OFFLINE=1` is rejected by cargo
   1.99.1 (`provided string was not true or false`); the design (§7.1) and this node's
   `# Acceptance` should say `CARGO_NET_OFFLINE=true`. Same for any future node copying the recipe.
2. **Offline `cargo metadata --locked` needs a Windows-only crate.** Unfiltered metadata (offline)
   fails downloading `anstyle-wincon v3.0.11`, which is already in HEAD's `Cargo.lock` but absent
   from the local cache — pre-existing, unrelated to this leaf. `--filter-platform
   x86_64-unknown-linux-gnu` passes. Either warm the cache or put the flag in the docs recipe.
3. **`blastit-py` has no compile-time `use` for the ingress types.** Its `src/**` is outside this
   leaf's `paths`; the dependency edge is wired and `cargo tree -p blastit-py -i blastit-ingress`
   proves importability. Whoever owns `crates/blastit-py/src/**` (Leaf MA.2.1.11/12) can add the
   `use` where `audit`/`probe_ingress` actually consumes the types.
4. **`blastit-ingress` declares its full §4.1 dependency set before it uses it** (`url`,
   `percent-encoding`, `thiserror`, `serde_json` are first consumed by MA.2.1.2). A
   `cargo-udeps`-style gate would flag them until then; the design mandates the exact set now.
5. **`RouteInfo::ingress` is unset everywhere.** `routes_from_manifest` keeps the fail-closed
   `None` until the manifest exposes `Route::ingress()` (MA.2.1.4); Leaf 6/10 wire
   `with_ingress`/`ingress_policy` at that point.
6. **No `LESSONS.md` entry:** `docs/**` is outside this leaf's `paths`, so the two cargo/offline
   gotchas above are recorded here for the test-curator to fold in.
