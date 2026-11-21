---
id: MB.40.10
title: "Fix MB.40.9 CI fallout: pinning.rs rustdoc, lexical boot_bindings + reload.is_some() gate, and /dev/shm tmpfs in PHP reload test"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [Makefile, "crates/blastit/**", "crates/blastit-view-php/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T12:11"
updated: "2026-11-19T12:19"
---

# Goal
Fix the 3 CI regressions exposed on `1a251a0`:
1. `rs-doc`: 3 private intra-doc links in `crates/blastit-view-php/src/pinning.rs:165,168,169` (`verify_entry`, `resolve_entry`, `pin_entry`), plus add `rs-doc` to `Makefile:fast-check` so rustdoc warnings are always caught locally.
2. `e2e rootless` (`e2e/rootless/php_entry_swap.py:331-350`): `boot_bindings` must keep lexical path resolution at cold boot (`manifest_dir.join(declared)` without `canonicalize`, so a cold restart while `public -> /tmp/entry-swap-decoy` is symlinked boots and fails closed `500` via `open_basedir` with 0 decoy opens rather than exiting at boot), and `Supervisor` must gate PHP repinning on `self.dev.is_some() && self.reload.is_some()` (not `self.dev_reload`) so when `fanotify` is unavailable (`self.reload == None`) or `--no-reload` is set, the boot pin is preserved.
3. `php-sapi.yml` (`make rs-test-php-image` in Docker): `a_dev_boot_reloads_a_php_save_in_place_by_rename_and_through_an_include` in `crates/blastit/tests/critical_php_embed.rs` placed its fixture in `/tmp` (which inside Docker is `overlayfs`, returning `EPERM` / `dev.reload.unavailable` on unprivileged `fanotify_mark`), instead of `/dev/shm` (`tmpfs`, where `fanotify_mark` succeeds inside Docker containers too!).

# Acceptance
- `make fast-check` (now including `rs-doc`!)
- `make rs-test-php`
- `uv run --no-project tools/plan.py validate`
