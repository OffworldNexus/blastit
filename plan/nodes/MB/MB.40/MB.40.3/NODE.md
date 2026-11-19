---
id: MB.40.3
title: "Deduplicate forks & seams: ObsVerdict, fold.py SQL FFI, single-impl traits, shared ELF & ProxySpec builders, delete mock_pg.rs"
kind: task
status: review
owner: implementer
priority: 1
depends_on: [MB.40.2]
paths: ["crates/**", "python/**", Cargo.toml, Cargo.lock]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T00:16"
updated: "2026-11-19T01:36"
---

# Goal
Eliminate cross-crate copy-paste forks, Python/Rust SQL-rule differentials, single-impl mock traits, cross-request mutable statics, duplicate ELF readers, duplicate `ProxySpec` boot loops, and `mock_pg.rs`.

# Exact Targets (`file:line` pre-diagnosed)
1. **Delete `ObsVerdict` fork (`crates/blastit-dbproxy/src/learn.rs:104-175`):** Use `blastit_sqlpolicy::LearnVerdict` directly (`blastit-dbproxy` already depends on `blastit-sqlpolicy`).
2. **Delete duplicate `escape_control` (`crates/blastit-env/src/error.rs:54`):** Import `blastit_manifest::escape_control` (`blastit-env` already depends on `blastit-manifest`).
3. **Eliminate Python SQL-rule differential (`python/blastit/fold.py:628-660`, `crates/blastit-py/src/lib.rs`):** Expose `has_quoted_literal` and `redact_literals` (or `check_learnable`) through `blastit._blastit` PyO3 module and delete `fold.py`'s re-derived `_has_quoted_literal`, `_redact_literal`, and `_unsafe_functions`!
4. **Remove single-impl mock-ceremony traits & cross-request static (`crates/blastit-http/src/server.rs:31` `Acceptor`, `crates/blastit-manifest/src/edit.rs:71` `Editor`, `crates/blastit-view-php/src/worker.rs:929` `RequestIo`, `crates/blastit-view-python/src/environ.rs:43` `static PORTS`):** Use concrete types directly (`Acceptor`, `Editor`, `RequestIo` have 0 fakes!) and replace `static PORTS: Mutex<Vec<...>>` in `environ.rs` with clean lock-free / per-worker caching or fast `itoa`/stack formatting.
5. **Unify the 2 hand-rolled ELF readers (`crates/blastit/src/host.rs:882-980` and `crates/blastit-view-php/src/discovery.rs:1153-1700`):** Extract one shared safe ELF reader module in `crates/blastit-sys/src/elf.rs` and call it from both `blastit` and `blastit-view-php`.
6. **Unify the 3 `ProxySpec` boot-wiring copies (`crates/blastit/src/stage0.rs:1773`, `stage0.rs:2182`, `crates/blastit/src/check.rs:671`)** and reconcile `mem_usage.rs` vs `blastit-sys/src/mem.rs` clearly.
7. **Delete `crates/blastit/tests/common/mock_pg.rs` (282 LOC):** Replace its test call sites with real `UnixStream` / `blastit-dbproxy` policy/framing tests (or ephemeral Postgres where needed), leaving **zero** mentions of `MockPgServer` or `mock_pg` anywhere in code or comments!

# Acceptance
- `make fast-check`
- `make rs-no-python rs-no-default`
