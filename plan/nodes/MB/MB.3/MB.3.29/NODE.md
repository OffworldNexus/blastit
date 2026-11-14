---
id: MB.3.29
title: "Toolchain::probe must not read a transient fork/exec failure as not-found (flaky critical_shim_contract)"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-php-sapi/src/toolchain.rs", "crates/blastit-php-sapi/tests/critical_shim_contract.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T19:46"
updated: "2026-11-14T19:46"
---

# Goal

`critical_shim_contract::an_unsupported_minor_is_refused_by_name` occasionally
reddens a full `make check` with `expected UnsupportedVersion, got NotFound`.

# Context

- `Toolchain::run` collapses **every** `Command::output()` failure into
  `NotExecutable` (`toolchain.rs:316-319`), and `discover_with` swallows
  `NotExecutable` (`Err(_) => {}`) before returning `NotFound`. A transient
  `fork`/`exec` failure under load (`EAGAIN`, `ETXTBSY`) therefore reads as
  "no toolchain installed".
- The protected test writes and execs a fresh `$TMPDIR/php-config` script
  (`FakeConfig`, pid+nanos name) per run, so a spawn failure is reachable.
- Observed by MB.3.23/MB.3.25/MB.3.13.1 reviews (passes in isolation and on
  retry; 15/15 isolated runs ok). Pre-existing from MB.3.12.

# Acceptance

```console
$ cargo test -p blastit-php-sapi --test critical_shim_contract --locked
$ make check
```
- A spawn/exec failure is distinguished from `ENOENT` (a typed `Spawn` error or
  one bounded retry), so "not found" only means "the kernel said ENOENT".
- The protected test cannot flake on a loaded host; the existing sentences and
  the no-toolchain degrade stay unchanged.
- Editing `tests/critical_shim_contract.rs` is authorized by this node
  (`critical_guard --node`).
