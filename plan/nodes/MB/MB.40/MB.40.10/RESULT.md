# MB.40.10 — MB.40.9 CI fallout: rustdoc links, lexical boot pin, tmpfs PHP reload test

## What changed

1. **`rs-doc` in the fast gate.**
   - `crates/blastit-view-php/src/pinning.rs`: the three private intra-doc links on
     `PhpBinding::repin_entry` (`verify_entry`, `resolve_entry`, `pin_entry`) are plain
     code spans now, so `RUSTDOCFLAGS="-D warnings" cargo doc` is clean.
   - `Makefile`: `fast-check` gains the `rs-doc` prerequisite (`guard critical-guard
     rs-fmt rs-lint rs-doc rs-test py-lint plan-validate`) and its help line, so a
     rustdoc warning is caught per slice instead of at the epic gate.

2. **Boot resolves lexically; only a live reload canonicalizes.**
   - `pinning.rs`: new `boot_entry` — `manifest_dir.join(declared)`, refusing an
     absolute string, a `..` component and a join outside the root **lexically**, with
     no `canonicalize` and no existence requirement at boot. `resolve_entry` keeps the
     canonicalize + confinement and now calls `boot_entry` first; it is reachable only
     from `repin_entry`.
   - Why: `e2e/rootless/php_entry_swap.py:331-350` restarts `blastit dev` while
     `/app/public -> /tmp/entry-swap-decoy` is a symlink and asserts the boot **starts**
     and answers `5xx` per request via PHP's `open_basedir` with 0 decoy opens. A boot
     that canonicalized refused to start.
   - New unit test `a_boot_pins_a_symlinked_entry_and_a_reload_refuses_to_move_the_pin_onto_it`
     proves both halves (lexical refusals, a symlinked `public/` that still boots and
     pins the inode at the declared path, and a reload that refuses to move the pin onto
     it).

3. **PHP repinning gated on the watcher, not on the promise.**
   - `supervise.rs`: the redundant `Parts::dev_reload` / `Supervisor::dev_reload` field is
     gone. `repin_php_bindings` and `spawn_now` gate on
     `self.dev.is_some() && self.reload.is_some()`, so `--no-reload` **and** a boot whose
     `fanotify_mark` could not run (`overlayfs` → `EPERM` → `dev.reload.unavailable`)
     both keep the boot `EntryPin`. `stage1/{boot,serve}.rs` no longer pass the field.

4. **PHP reload leg on `tmpfs`.**
   - `crates/blastit/tests/critical_php_embed.rs`: `Scratch::on_tmpfs` (writable
     `/dev/shm`, else `temp_dir()`), used by
     `a_dev_boot_reloads_a_php_save_in_place_by_rename_and_through_an_include`; the
     escape fixture is `Scratch::outside_file`, a sibling of `scratch.dir` on the same
     tmpfs, removed on drop. Inside `make rs-test-php-image`'s container `/tmp` is
     `overlayfs` (`EPERM`) and `/dev/shm` is `tmpfs` (mark succeeds).

## Evidence

See the verification tails pasted by the run below: `make fast-check` (with `rs-doc`),
`make rs-test-php` (real host `libphp8.4` + shim, live-reload leg included) and
`make rs-no-python`.

## Follow-ups

- `crates/blastit-view-php`'s own unit test `a_repin_records_the_saved_entry_and_refuses_one_that_left_the_root`
  still plants its escape fixture in `std::env::temp_dir()`; it is a plain file, not a
  watched tree, so it needs no tmpfs.
