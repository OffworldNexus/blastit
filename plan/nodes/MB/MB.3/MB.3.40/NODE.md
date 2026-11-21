---
id: MB.3.40
title: "Production provenance gate for libphp/shim/manifest_dir (R7 + B2-R5) with protected legs"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/src/stage0.rs, crates/blastit/src/host.rs, crates/blastit/tests/critical_php_embed.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T01:42"
updated: "2026-11-15T02:48"
---

# Goal

Implement and protect the production provenance half of `ADR-0010` §3 / design `R7`
for the embedded PHP pair. `MB.3.13.3`'s follow-up records it as missing:

> Production provenance is still `MB.3.13.2`'s half: `ADR-0010` §3 and design
> `R7`/`B2-R5` ask the gate to refuse a `libphp`/shim whose parent a route uid could
> write, plus a writable `manifest_dir`/`entry`, via `host::plain_is_protected(path,
> init_uid)` and `!has_file_caps(path)` — also "zero writable `DT_RPATH`/`DT_RUNPATH`".
> Today the gate inspects the ELF symbols and the tuple only.

The primitives already exist and are wired for the plain executable only
(`crates/blastit/src/host.rs:273` → `plain_is_protected` at `:431`, `has_file_caps`
at `:315`). Stage 0's PHP gate (`stage0.rs::php_boot_gate`) must apply them, in
`ExecutionMode::Production` only, to:

1. the resolved NTS `libphp` and the selected shim — `plain_is_protected(path,
   init_uid)` and `!has_file_caps(path)`, plus zero `DT_RPATH`/`DT_RUNPATH` entries
   pointing into a directory a route uid can write (read the dynamic section the
   same way `zend_abi()` already parses `.dynsym`; no `dlopen`, no new dependency);
2. `manifest_dir`, the route's `entry`, and `static.root` — not writable by any
   route uid/gid (`B2-R5`), because the worker's `cwd`/`include_path` is that tree;
3. the refusal sentence keeps `ADR-0010` §4.1's tuple shape, names the offending
   path and the fix, and still refuses before any listener binds.

`dev`/`check` must not apply it (the shim lives in a writable checkout there), and
the reason must be in the code, not only in the test.

Then protect it — this is the acceptance, not the code:

- `crates/blastit/tests/critical_php_embed.rs`: a stub pair and a manifest whose
  `manifest_dir` (or the shim's directory) is `0777` under a fake `Production` boot
  refuses with the provenance sentence and starts no listener. Where a real
  privileged boot is impossible in `make check` (no root), drive the gate through
  the same seam the existing gate unit tests use and assert the *sentence and
  order*, and say in `RESULT.md` which half is end to end and which is unit-level.
- The accepted twin: the same fixture with `0755`/root-owned directories boots and
  serves (the permission bits, not the path, are the decision).
- A `DT_RPATH` fixture: an ELF whose dynamic section names a world-writable
  directory is refused; the same ELF with no `DT_RPATH` is accepted.

# Context

- `docs/testing/AUDIT-MB.3.13.md` gap G6; design `backend-runtimes.md` line 318
  lists `DT_RUNPATH` + `/tmp/evil.so` + writable `extension_dir`/`manifest_dir`/
  `entry`/`static.root` + `opcache.preload` as **critical** requirements of the
  `R6`/`R7`/`R8` row, and none of them has a test today.
- `ADR-0010` §3 also pre-scans PHP `.ini` / `extension_dir` / `extension=` /
  `zend_extension=` targets. If that is more than this leaf can carry, implement
  (1)–(3) here and file the `.ini` scan as an explicit follow-up with the reason.
- `MB.3.18` owns the matrix; this node is the gate rule, not the matrix.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
test a_world_writable_manifest_root_refuses_a_production_boot ... ok
test a_world_writable_shim_directory_refuses_a_production_boot ... ok
test a_runpath_into_a_writable_directory_refuses_a_production_boot ... ok
test the_same_tree_owned_by_init_is_accepted ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- Removing any one of the three checks makes its leg fail (paste the red run).
- A `dev` boot of the same `0777` fixture still serves — the rule is
  `Production`-scoped (`ADR-0010` §3, `R7`), and the transscript for that twin is in
  `RESULT.md`.
- `make check` exits `0`; `cargo clippy --workspace --all-targets --locked
  --features python -- -D warnings` is clean (no new `unsafe`, no new dependency);
  `critical_guard.py --node <this id>` reports the protected edits as authorized.
- The refusal is `exit 1`, nothing on stdout for a stage-0 refusal, and no
  `ready`/`process.spawned`/`request` event.

# Notes

- Never load the object to inspect it: parse the dynamic section statically, as
  `zend_abi()` already does for `.dynsym` (zero `dlopen` is the `R11` invariant).
- The `(st_dev, st_ino)` record and its stage-1 pin stay exactly as they are; this
  node adds an authorization check in front of them.
