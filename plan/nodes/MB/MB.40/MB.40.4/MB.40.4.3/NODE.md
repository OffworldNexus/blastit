---
id: MB.40.4.3
title: Split blastit stage0.rs/stage1.rs into cohesive modules + fork/exec parameter structs
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.40.4.2]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - "crates/blastit/src/stage0/**"
  - "crates/blastit/src/stage1/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T03:13"
updated: "2026-11-19T04:04"
---

# Goal

Behaviour-preserving split of the two process-plane files — `crates/blastit/src/stage0.rs` (5,043 lines / ~1,601 prod LOC, 58 free fns) into `stage0/{mod,gates,fork_exec,dev}.rs` and `crates/blastit/src/stage1.rs` (3,003 / ~1,990, 40 free fns) into `stage1/{mod,adopt,spawn,python_env}.rs` — and grouping the >6-arg functions (`fork_scheduler` 11, `fork_http` 8, `exec_stage1_dev` 8) into cohesive parameter structs. Every resulting production file must stay under 500–600 prod (non-blank, non-comment) LOC.

This is a **pure refactor**: boot order, fd/`OwnedFd` ownership order, privilege transitions and every boot-refusal sentence stay literal.

# Exact Targets (file:line pre-diagnosed)

Line numbers are from the pre-split files at `MB.40.4` closure. Do not re-audit.

## `stage0.rs` → `stage0/`

- **`mod.rs`:** module docs/imports, `Early` `:83`, `fail_early` `:106`, `current_uid` `:70`, `stage0_lifecycle` `:113`, `config_lifecycle` `:131`, `warn_about_siblings` `:157`, `others_with_uid` `:189`, `effective_uid` `:208`, `privilege_from` `:213`, `capability_advice` `:241`, `copy_recipe` `:278`, `pub fn run(host, interpreter, preset)` `:1013-1275`, `reexec_argv` `:2469`, `program` `:2481`, `cstring` `:2487`.
- **`gates.rs`:** `static_app_refusal` `:309`, `is_static_only` `:323`, `runs_own_executors` `:334`, `app_rule` `:351`, `raw_ingress_gate` `:378`, `no_python_support` `:445`, `probe_timeout` `:517`, `config_document` `:587`, `php_boot_gate` `:645`, `object_is_protected` `:679`, `tree_is_protected` `:694`, `php_boot` `:711`, `deployment_boot_check` `:735`, `no_php_support` `:775`, `emit_runtime` `:804`, `runtime_shape` `:823`, `python_runtime` `:845`/`:874` (two `cfg` variants), `static_root_permission` `:887`, `listable` `:930`, `static_root_refusal` `:940`, `layout_collision` `:956`, `dbproxy_limits` `:992`, `route_declares_proxy` `:2120`.
- **`fork_exec.rs`:** `fork_dbproxy` `:2139`, `DbproxyHandover` `:2284`, `wait_ready` `:2305`, `emit_child_output` `:2343`, `exec_stage1` `:2385`, `pipe_ok` `:2495`, `fork_ok` `:2507`.
- **`dev.rs`:** `DevBoot` `:1276`, `dev_fail` `:1288`, `fail_dev_root` `:1296`, `memory_reading` `:1313`, `memory_refusal` `:1318`, `fail_with_root` `:1323`, `pipe_dev` `:1330`, `cstring_dev` `:1339`, `reexec_argv_dev` `:1348`, `pub fn run_dev` `:1368-1649`, `dev_policy_channel` `:1650`, `dev_view_sockets` `:1668`, `fork_dbproxy_dev` `:1762`, `exec_stage1_dev` `:1892`, `bind_view_sockets` `:2024`.
- **Arg-struct target:** `exec_stage1_dev` `:1892` currently takes 8 arguments → one cohesive params struct (≤6 args after grouping). `fork_scheduler` (11) and `fork_http` (8) live in `stage1.rs` (below).

## `stage1.rs` → `stage1/`

- **`mod.rs`:** `Options` `:94`, `pub fn run(options, interpreter)` `:242-851`, `Serving` `:884`, `serve_without_interpreter` `:852`, `after_preload_marks` `:987`, `serve` `:998`, `learn_routing_table` `:1120`, `emit_ready` `:2109`, `fail` `:2124`, `python_fail` `:2134`, `ViewChildPlans` `:2173`, `mod tests` `:2403` (stays in `tests.rs`/`mod.rs` next to its subjects).
- **`adopt.rs`:** `read_config` `:1151`, `BootConfig` `:1135`, `member` `:1334`, `number` `:1342`, `open` `:1351`, `stage1_program` `:1366`, `dev_root_from_cmdline` `:1398`, `test_swap_pause` `:1424`/`:1435`, `Boot` `:1637`, `load` `:1656`, `check_view_paths` `:1816`, `immortalize` `:1987`, `preload_lifecycle` `:2046`, `import_callable` `:2097`, `current_usage` `:1524`, `read_memory` `:1532`, `refresh_memory` `:2013`, `emit_preload` `:2026`.
- **`spawn.rs`:** `http_config` `:1443`, `scheduler_config` `:1463`, `fork_http` `:1554` (8 args), `fork_scheduler` `:1578` (11 args), `prepare_child` `:1609`, `php_slots` `:2201`, `php_max_requests` `:2265`, `php_max_body_bytes` `:2275`, `emit_php_runtime` `:2297`, `view_child_plans` `:2331`, `component_limits` `:2369`, `pipe_ok` `:2378`, `pair_ok` `:2390`.
- **`python_env.rs`:** `put_cwd_first` `:1914`, `restore_sys_path` `:1958`.
- **Arg-struct target:** `fork_scheduler` `:1578` (11 args) and `fork_http` `:1554` (8 args) → one `ForkRequest`-style params struct each (or one shared struct if the fields genuinely coincide), taking them to ≤6 args.

# Hard constraints

1. **Process plane is literal.** `OwnedFd`/`RawFd` open→dup→fork→exec ordering, `close`/drop points, and the handover struct field order must be moved verbatim; no re-ordering, no early-drop, no `Drop` impl added.
2. **Boot-refusal sentences unchanged.** Every `String` returned/printed by the gates (`static_app_refusal`, `app_rule`, `raw_ingress_gate`, `deployment_boot_check`, `php_boot_gate`, capability advice, `dev_fail`) is asserted by tests and docs — copy byte-identically.
3. **Public surface unchanged.** `crates/blastit/src/lib.rs:81-82` (`mod stage0; mod stage1;`) must keep resolving with no edit, and `stage0::run`, `stage0::run_dev`, `stage1::run`, `stage1::Options`, `deployment_boot_check`, `DbproxyHandover`, `ViewChildPlans` keep their exact paths, visibility and signatures (arg-struct grouping applies only to the three private >6-arg fns; if `fork_*` proves reachable from tests, add a struct that preserves the call shape).
4. **argv/contract untouched.** The `--config-fd` JSON keys, `stage1_program` argv order and `reexec_argv{,_dev}` output order are the re-exec contract — unchanged.
5. **No protected tests involved.** `crates/blastit/tests/critical_*.rs` (including the no-mount refusal tests) need no edit and must not be touched.
6. **No new globals**, no new traits, no `unwrap()`/`expect()` on untrusted input.

# Acceptance

```
make fast-check
make rs-no-python rs-no-default
cargo test -p blastit --locked
cargo clippy -p blastit --all-targets --locked -- -D warnings
cargo fmt --all -- --check
for d in crates/blastit/src/stage0 crates/blastit/src/stage1; do
  for f in $d/*.rs; do printf '%s %s\n' "$(awk 'NF && $1 !~ /^\/\// ' "$f" | wc -l)" "$f"; done
done
```

Report tails in `RESULT.md`, plus `git status --porcelain` proving only `stage0.rs`/`stage1.rs` → directories changed, `grep -c "" crates/blastit/src/lib.rs` unchanged, and `git diff crates/blastit/tests` empty.
