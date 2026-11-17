# Security review — `view-crates-architecture` (sign-off pass, round 9)

- **Lens:** security (zero-trust, privilege/sandbox identity, CVE resistance, artifact/distribution)
- **Subject:** `docs/design/view-crates-architecture.md` (1 001 lines), `docs/decisions/ADR-0016-view-crates-over-wire.md` (138 lines)
- **Tree:** `.worktrees/release-0.2.0`, 2026-11-17. Claims re-checked against `file:line` where a claim is executable.
- **Verdict:** **0 BLOCKER, 0 MAJOR.** The round-8 MAJOR (`PhpProbeBoot` carrying one `Privileges`/`Filter` for N probe children) is fixed correctly: `PhpSlot` now carries its own `privileges`/`filter` (467-474) and `PhpProbeBoot { slots, bindings, timeout }` (484) has no global mode left to mis-apply. The per-slot shape matches the tree's one-child-per-slot probe (`crates/blastit/src/php_view.rs:605-621`), and no view crate names `Shared`/`Off` (census scope, §4.4 D1 391-395; §7 R4 734).

| Severity | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 0 |
| MINOR | 6 carried + 2 new (non-gating) |

## 1. The eight invariants, item by item

| # | Invariant | Evidence in this revision | State |
|---|---|---|---|
| 1 | Stage-0 `execve` secret barrier; no secret in a route's `environ`/`argv`/heap | §5 row 1 (666) keeps `stage0.rs`'s secret half untouched; no new `BLASTIT_*`; `SpawnContext` is a struct, `loaded` rides `ChildIo.keep` (455-458). The one thing the move could have broken is the probe: `probe_bindings` is called from **stage 1**, after the barrier (`crates/blastit/src/stage1.rs:395`), so a probe child forks a heap that never held a credential. `enter` runs after the barrier and reads no mode (§4.4 D1 384-386). Oracles: `critical_dev_and_fail_closed::dev_boots_five_unprivileged_roles_and_strips_secrets`, `smoke.rs`'s environ case (666). | **holds** |
| 2 | Roles/order/three forks | `enter` is steps 1-7 in one order (§4.4 D1 368-372), `setpgid` inside and only the probe sets `pgroup: true` (399-404) — required because `EmbeddedView` answers `setpgid` with `EPERM`; exactly three `blastit_sys::child::enter(plan)` call sites (`zygote.rs`, `spawn.rs`, `probe.rs`), with R4's two censuses (calls == 3, hardened-call == 0) (734). `SeccompRole::{View,EmbeddedView}` unchanged; filter after drop and after `setpgid`, before the first thread (96-99, 667). New `critical_child_sequence` has **five unprivileged** cases (667, 734), the drop proved by a `DropTo(other uid)` failing before `READY` rather than a root runner. | **holds** |
| 3 | PHP pinning; no `dlopen` above the worker; nothing mapped | `(st_dev, st_ino)` library/shim pins re-opened `O_RDONLY|O_CLOEXEC|O_NOFOLLOW` and `dlopen`ed as `/proc/self/fd/<n>`; opaque `EntryPin` (no `Option`, no `Default`) required by `PhpRuntimeConfig::new`, verified at load and before every request, `PinnedEntryRuntime` deleted (R6 104-108, §5 669). `discovery` names no SAPI/`dlopen`, with a call-anchored grep + clippy `disallowed_path` + the `critical_php_embed` canary (R5 102-103, 735). The init-image limitation is recorded, not hidden (D5 561-564). | **holds** |
| 4 | `openat2`-only; one decoder; `.php` never served | `STATIC_RESOLVE_FLAGS` unchanged, `resolve(&self, &[&str])` cannot produce a path, the scheduler decodes once (§4.4 D3 519-530), the static crate's only decoder text is the `use` + the named `head_segments` test mirror (R7 109-114, 737). The round-2 `debug_assert!` stays removed, with the `%25`/`%252e` reason recorded (953, 531-534) — no panic on a client target. `is_php_source` applied twice before any descriptor (671). | **holds** |
| 5 | `view.chan` credit window | Frames, `INITIAL_WINDOW`, low-water mark, `CANCEL`/`ABORT`, stale-frame rules byte-identical (R10 120-122); `Credit::grant` public but documented as *applying* a scheduler-sent `WINDOW`, with the scheduler's accounting named as the bound (210, 672, 955). The public type creates no new authority: the worker's transport thread already parsed `WINDOW` there, and R10 pins the bytes. | **holds** |
| 6 | Transport purity | `blastit-view` has no `pyo3` (incl. `[dev-dependencies]`) and no `blastit-php-sapi`, no `runtime.rs`, `default = []` (R2 85-89; R13 127-130). The manifest grep is run *inside* `make rs-no-python` because `cargo tree -e normal` is blind to dev-deps (732) — the right oracle. | **holds** |
| 7 | Artifact linkage | `-p blastit` defaults `php,static`, `NEEDED` = libc family only, no `libpython`/`libphp`, no `RPATH`/`RUNPATH`; wheel = `python,php,static` with the python capability and never a `libphp` `NEEDED`; `--workspace` banned from build/ship commands (R11/R12 123-126; §4.3 306-312; §5 675). `make rs-no-default` builds R13's corner that nothing builds today (333-335). | **holds** |
| 8 | Fail-closed production | `PrivilegeMode`/`caps` untouched; `BLASTIT_SECCOMP=0` remains the only filter switch (and stays refused under a preset, per the tree) carried as `Filter::Off` from stage 0's `config.seccomp`; stage 1 is the only builder of `Shared`/`Off` and a `blastit` unit test pins the production pair (383-396, 674). Oracles: `critical_dev_and_fail_closed`, `critical_config_and_presets`, `critical_embedded_seccomp`. | **holds** |

CVE classes: unchanged by construction — no ingress code, parser or decoder moves (§5 707-712). `php2shell`'s surfaces (`ZEND_INI_SYSTEM` lock, `disable_functions`, `open_basedir`, `SCRIPT_FILENAME`/`DOCUMENT_ROOT` pins, no argv/FastCGI path) move verbatim inside `blastit-php-sapi` + `worker.rs`; `node2shell`/polyglots are later crates that this seam exists to keep separate.

## 2. BLOCKER / MAJOR findings

**None.** Nothing in this revision lets a route read a secret, escape its uid, skip the filter, widen a decoder, or boot a production process with fewer privileges than today.

## 3. Carried MINORs from round 8 (unchanged, non-gating)

1. `blastit_sys::set_env` is still a **safe** fn around edition-2024-unsafe `std::env::set_var` with a doc-only single-threaded precondition (§5 677-683). The precondition is honest today (fresh child, pre-thread: `crates/blastit/src/php_view.rs:1158-1165`) but the API is callable from a threaded context, which is the future Node/JVM view kind. Cheapest fix in the same leaf: move `HOME`/`TMPDIR` into `ChildPlan.env` and apply them inside `enter` (where `unsafe` already lives), deleting the public helper; keep the existing NUL rejection on the path (`php_view.rs:1152-1157`), since `set_var` panics on an interior NUL.
2. Resolution-table rows 973 (`PhpProbeBoot { …, privileges, filter, … }`) and 966/979 still describe superseded shapes; §4.4 (484) and §7 R4 (734) are normative. Mark them superseded, one clause each.
3. `ChildIo::prepare` still resolves the cwd by path, as root, before the drop (401-407). `O_NOFOLLOW` closes the symlink swap; carrying the directory as an `O_PATH` fd in `ChildIo.keep` would close the race entirely.
4. `critical_child_sequence` must say each case runs **in a forked child**: `install_filter` is irreversible and `View`/`EmbeddedView` deny `fork`/`execve`, so an in-process case wedges `libtest` for the rest of the suite. *Fix:* one sentence in §7 R4.
5. D1's census and R17's `unsafe` guard still name no expected file set / count (391-395, 747). Name `crates/blastit/src/stage1.rs` as the only permitted `Shared`/`Off` site (plus the `BLASTIT_SECCOMP=0` branch) and the guard's one `#[allow(unsafe_code)]` in `blastit-view-python/src/immortal.rs`.
6. `Deployment::is_protected` is still an injectable `&dyn Fn` (490-494). In production there is one construction path (`stage0.rs`'s two wrappers) and the protected suite drives the real binary over a world-writable manifest dir / front controller / shim dir and a `RUNPATH` (`crates/blastit/tests/critical_php_embed.rs:3582-3893`), so the fail-open is test-covered; a named trait with one implementation would make a permissive closure something someone writes on purpose.

## 4. New MINORs from the per-slot probe change (non-gating)

7. **`uid` now has two carriers.** `PhpSlot.privileges = DropTo(uid)` (472) and `PhpBinding.uid` (487). The drop uses the first, the refusal sentence names the second; a stage-1 mismatch makes the probe refuse in the wrong route's name. Fix by deriving the sentence's uid from the same `Privileges` value the child actually received (or drop `PhpBinding.uid`).
8. **`slots`/`bindings` length is unchecked** (`probe_bindings`, 482-484). Misalignment probes binding *j* in slot *k*'s uid/cwd — fail-closed in practice (the entry is unreadable from the wrong uid/open_basedir) but makes the boot proof non-authoritative. One `if slots.len() != bindings.len() { return Err(..) }` at the top.

## 5. What passes without action

The round-8 ACL result this pass was re-checking: no view crate constructs `Privileges::Shared`/`Filter::Off` (typed copy from `SpawnContext`/`PhpSlot`), the production constructor is pinned by a unit test, `enter` fails before any load and every caller exits non-zero (`child_exit`), `set_dumpable` is documented as best-effort with the `execve` barrier as the real mechanism, and the two behavioural deltas (symlinked scratch dir refused; `HOME`/`TMPDIR` after the mask restore, both after the drop) are stated rather than implied.

VERDICT: PASS
