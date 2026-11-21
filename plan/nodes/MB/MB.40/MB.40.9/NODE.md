---
id: MB.40.9
title: "Fix PHP live reload EntryPin repinning (in-place + atomic save + boot watch) & polish dev banner/static/phase UX"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit/**"
  - "crates/blastit-view-php/**"
  - "crates/blastit-scheduler/**"
  - "crates/blastit-events/**"
  - "docs/**"
  - README.md
  - "python/tests/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T11:25"
updated: "2026-11-19T11:52"
---

# Goal
Make `blastit dev` live reload actually work seamlessly on embedded PHP apps (both in-place writes and atomic `rename` saves of `entrypoint php` and included `.php` files, from boot before first request and across reloads without `500 changed since this boot recorded it`), while keeping production / `--no-reload` `EntryPin` confinement strict, and polish the 3 UX warts exposed in the real PHP `blastit dev` session (`view` names in banner, `static` route target instead of `(unmatched)`, and 200ms-gated `dbproxy` wait line).

# Root Cause & Exact Targets (`file:line` pre-diagnosed)
1. **PHP Live Reload Repinning & Boot Watch Seeding (`crates/blastit-view-php/src/pinning.rs`, `crates/blastit/src/supervise.rs`, `crates/blastit/src/stage1/boot.rs`, `crates/blastit/tests/critical_php_embed.rs`, `crates/blastit/tests/critical_dev_reload.rs`):**
   - **Why it broke:** `Supervisor::reload_all` (`supervise.rs:864`) emitted `dev.reload` and sent `Ctl::Reload` retiring PHP workers, **without** refreshing `self.php` (`Vec<Option<PhpBinding>>`), so newly spawned PHP workers inherited the boot-time `EntryPin` (`st_dev, st_ino, st_size, st_ctime`) and `500`d every request with `"entrypoint php ... changed since this boot recorded it"`! Additionally, `stage1` did not seed `PhpBinding::entry` paths into `ReloadPolicy` at boot, so editing `public/index.php` before the first request wasn't watched until after request 1!
   - **Fix 1a (`pinning.rs`):** Add `PhpBinding::repin_entry(&mut self, manifest_dir: &Path, declared_entry: &str) -> Result<(), String>` which re-runs `resolve_entry(manifest_dir, declared_entry, ...)` (enforcing `manifest_dir` confinement & symlink-escape refusal!) + `pin_entry(...)` and updates `self.entry`, `self.entry_pin`, and `self.entry_fd` atomically!
   - **Fix 1b (`stage1` boot):** When building `Reload` at stage-1 boot in `blastit dev` (when `dev_reload` is on), seed every `Some(binding)`'s `binding.entry` (and `document_root`) into `Reload` (`record_loaded` / initial watched files) so `public/` is marked by `fanotify` at boot even before the first HTTP request!
   - **Fix 1c (`supervise.rs`):**
     - In `Supervisor::reload_all` (when `self.dev.is_some() && self.dev_reload`), before `self.send_ctl(&Ctl::Reload)`, call `repin_entry` on every `Some(binding)` in `self.php` (using the manifest's declared `entrypoint php`). If `repin_entry` fails (e.g. file deleted mid-save or symlink escape outside `manifest_dir`), emit `self.warn(err)` and keep the old pin so confinement never fails open!
     - Also on `Ctl::Spawn` for a PHP slot when `self.dev.is_some() && self.dev_reload`: call `repin_entry` before `fork_php_view` if the entry's metadata on disk differs from `binding.entry_pin`, AND if an active PHP worker exists while `fanotify` hasn't debounced yet, remember `reload_all` retires active workers after debounce (~50ms) and `Spawn` repins immediately for cold workers!
   - **Fix 1d (`critical_php_embed.rs` & real PHP live-reload test):**
     - In `crates/blastit/tests/critical_php_embed.rs` (`Boot::launch` or the `EntryPin` decoy/in-place refusal legs at `:3225+`), pass `--no-reload` on the pin-refusal legs (since those legs test the static boot-pin security invariant when live reload is off, just like production!).
     - Add a **real-pair PHP live-reload test** (executed by `make rs-test-php` on this host!) proving:
       1) Boot `blastit dev` (default reload ON) on a PHP app -> `GET /` returns `v1` (`200`),
       2) Edit `public/index.php` **in place** (`v1` -> `v2`) -> wait for `dev.reload` -> `GET /` returns `v2` (`200`, ZERO `500`!),
       3) Replace `public/index.php` via **atomic `rename`** (`v2` -> `v3`, new inode!) -> wait for `dev.reload` -> `GET /` returns `v3` (`200`, ZERO `500`!),
       4) Edit an **included** `src/helper.php` (`require_once`) -> wait for `dev.reload` -> `GET /` returns `v4` (`200`),
       5) Replace `public/index.php` with a symlink pointing **outside** `manifest_dir` -> `repin_entry` refuses with a `manifest.warning` and does NOT execute the outside file!
2. **Banner View Names & Static Mount Rows (`crates/blastit/src/dev_learn.rs:100-110`, `crates/blastit/tests/critical_dev_and_fail_closed.rs:1226-1275`, docs/runnable-docs pins if any):**
   - Instead of printing `  view  GET, HEAD  /up` with no route name 13 times, render aligned columns naming each view (and any `static` mounts!):
     `  view   <name>  <methods>  <patterns>` (and `  static <prefix>  GET, HEAD  <root>` when `manifest.mounts()` is non-empty!). Update `dev_learn.rs` unit tests and authorized `critical_dev_and_fail_closed.rs:1226-1275` assertions cleanly.
3. **Static Asset Request Log Target (`crates/blastit-scheduler/src/scheduler/`, `crates/blastit/tests/dx-parity.*` if affected):**
   - When a `static` mount handles a request (`200`, `304`, `404` inside static mount), set `event = event.with_route("static")` (or `format!("static {}", mount_prefix)` — check `dx-parity.txt:10` and `critical_*` tests first so everything agrees!) instead of leaving `event.route = None` which rendered `GET /css/app.css → (unmatched)  304`! Genuinely unmatched requests (`Outcome::Unmatched` with no static mount) still render `(unmatched)`.
4. **Delay `dbproxy` Wait Phase Line by 200ms (`crates/blastit/src/stage0/fork_exec.rs:131,420`):**
   - Poll `ready_read` for `200ms` first; if `dbproxy` is already ready within `200ms` (the normal fast path!), emit no noisy `"  dbproxy    waiting for the ready byte"` line above the banner! Only if `200ms` elapses without readiness, emit `phase(emitter, "dbproxy", "waiting for upstream database...")` and wait the remaining `DBPROXY_READY_TIMEOUT`! (Keep `phase_line` unit test in `logs.rs` green.)

# Acceptance
- `make rs-test-php` (proves real PHP 8.4 SAPI shim + `critical_php_embed` + `critical_php_backend` + `critical_parallel_views` + new PHP live-reload legs!)
- `make fast-check`
- `make docs-check docs-test`
