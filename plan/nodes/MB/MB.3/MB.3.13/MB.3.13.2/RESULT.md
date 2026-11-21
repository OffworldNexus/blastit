# RESULT MB.3.13.2

STATUS: done.

## Review MINORs (all three closed)

1. **Every `[route.php]` route's `workers` is now checked**, in manifest order,
   by `php_worker_budget(manifest, cap)`; the first route above the budget is
   refused naming *that* route and both numbers. A two-route manifest whose
   **second** route is `workers = 20` against a budget of 16 now gets
   `route "api": [route.php] workers = 20 is above this deployment's per-view
   budget of 16 (…)` (CLI-verified), while `workers = 16` on that same second
   route is an accepted twin. New test:
   `a_workers_cap_above_the_view_budget_names_both_numbers` carries both.
2. **The version-less refusal keeps the `ADR-0010` §4.1 tuple shape**: the
   `present_refusal` `None` arm now prints `(PHP ?, <arch>-<libc>)` and says no
   shim can be chosen until the object's version is known. Asserted by the new
   `a_version_less_refusal_still_names_the_tuple_with_a_question_mark` (a text
   file named `libphp.so`). `PhpRefusal::NotAbsolute` is **kept, not dropped** —
   `php_boot` is a pure function with an injectable search, so it is defence in
   depth behind `Config::validate` — and it now has its own unit test,
   `a_relative_libphp_is_refused_by_the_gate_too`, which asserts the message
   names `BLASTIT_LIBPHP`, "not an absolute path" and the package.
3. **Hand-off comment added**, not code: at the shim lookup in
   `discover_php`, saying `make php-sapi` still writes
   `target/php-sapi/libblastit_php84.so` while selection asks for
   `libblastit_php84-<arch>-<libc>.so`, that the rename is
   `MB.3.13.3`/`MB.3.18`'s logged hand-off, and that this lookup deliberately
   accepts one spelling per tuple.

Re-ran after the fixes: `cargo test -p blastit --locked` (178 lib tests, all
suites green) and `make check` (`make_exit=0`).

## Lead correction (applied)

Item 6 of the brief was wrong: `MB.3.28`'s interim refusal stays, sentence and
call sites unchanged, and **this node's gate runs in front of it**. Both boot
paths now apply the order the lead gave:

1. **`workers > view_worker_cap()`** — a manifest rule, naming both numbers
   (`workers = 20` vs `budget of 16`), accepted twins `== cap` and no `workers`.
   Nothing about the host can fix it, so it comes first.
2. **`libphp` discovery + static NTS/minor check + shim selection** — with
   `ADR-0010` §4.1's two exact sentences, so a host that cannot embed PHP is
   told to install the package or set `BLASTIT_LIBPHP`, never sent to a wiring
   excuse.
3. **`MB.3.28`'s interim refusal, verbatim** — `route "app": the embedded PHP
   backend is not available in this build ([route.php] entry
   "public/index.php"); the per-route worker wiring lands in MB.3.13.3` — so a
   host that *can* embed PHP still fails closed instead of binding a route this
   build has no worker for (the WSGI-fork hazard `.28` exists to prevent).

One scope rule makes steps 2 and 3 compatible with the protected suite, which
`blastit dev` runs on a PHP-free host (CI installs no PHP):

- `ExecutionMode`/`PhpShimScope::Deployment` (`blastit`): every discovery
  refusal is fatal — missing library, a directory, not an ELF, ZTS, no version,
  no shim.
- `PhpShimScope::Development` (`blastit dev`): only a library that **is there
  and is wrong** is fatal (not a regular file, not an ELF, ZTS, no version in
  its name). "Nothing installed" and "no shim built for the tuple" are left to
  step 3 — which is fatal too until `.13.3`, and whose sentence is the true one
  for a `dev` boot that would refuse even after `make php-sapi` and
  `apt install libphp-embed`. Sending a developer to install a package for a
  boot that still cannot serve is the confusion
  `docs/design/php-dev-local.review-ai-dx.md` rejects.
- `blastit check` keeps its single call site and therefore step 3 only
  (`check.rs` is out of `paths`; see follow-ups).

`git diff` confirms the interim sentence and its three call sites are as
`MB.3.28` left them; nothing under `tests/` was touched, and `make check` is
green with both suites unmodified.

## What was delivered

`crates/blastit/src/stage0.rs` (the gate), `crates/blastit/src/config.rs` (the
settings and the resolved runtime), `docs/reference/settings.md` (one row).
`crates/blastit/src/cli.rs` needed no change: `check` already exits `2` through
`check::execute`'s `Err`, and the `--json` "zero bytes on stdout" rule is the
existing `main` behaviour (proved below).

1. **Discovery.** `BLASTIT_LIBPHP` (absolute only; `open` + `fstat`, regular
   file) when set, otherwise the fixed list in `LIBPHP_CANDIDATES` order —
   `libphp8.4.so`, `libphp-8.4.so`, `libphp8.3.so`, `libphp-8.3.so`,
   `libphp8.2.so`, `libphp-8.2.so`, `libphp.so` — each looked for in
   `library_dirs()` order: `/usr/lib/<arch>-linux-<libc>`, `/usr/lib64`,
   `/usr/lib`, `/usr/local/lib/<arch>-linux-<libc>`, `/usr/local/lib`.
   **Name-major**: a newer version in `/usr/local/lib` wins over an older one in
   `/usr/lib`. Zero `dlopen`: the gate only `open`s, `fstat`s, `seek`s and
   `read`s (proved by `the_gate_reads_the_elf_and_never_loads_it`).
2. **NTS, statically.** `zend_abi()` parses the ELF header, walks the section
   header table to the `SHT_DYNSYM` section, reads its linked `SHT_STRTAB` and
   scans the symbols: `executor_globals` present **and** `tsrm_startup` absent
   → NTS; `tsrm_startup` present → ZTS refusal; neither → "not a PHP embed
   library". ELF32/ELF64 little-endian only; every offset bounds-checked
   (`le_u16/32/64`, `read_at`, `bounded_section`, 64 MiB section cap). The
   `major.minor` comes from the file name (either distro spelling, `libphp.so.0`
   sonames included) or from the name the open descriptor resolves to
   (`/proc/self/fd/<n>` `readlink`, which is how a `libphp.so` symlink still
   names its version). No version, no shim: a refusal, never a guess.
3. **Shim selection.** `libblastit_php<major><minor>-<arch>-<libc>.so` for
   `(host major.minor, cfg!(target_arch), target_env gnu|musl)`, looked up in
   `BLASTIT_PHP_SHIM_DIR` (dev/check only, `R8`), then beside the executable,
   then `<workspace>/target/php-sapi` derived from the running executable's
   `target` ancestor (dev/check only). No shim for the tuple is a refusal naming
   the tuple, the shim's file name, every directory searched, the package and
   `BLASTIT_LIBPHP`.
4. **The two exact messages** (`ADR-0010` §4.1) — verbatim below.
5. **`workers > view_worker_cap()`** — see step 1 above.
6. `php_backend_unavailable` restored **verbatim** (`git diff` shows no change
   to its sentence or its three call sites); the discovery gate is called first
   in `run` and `run_dev`.
7. `BLASTIT_LIBPHP` and `BLASTIT_PHP_SHIM_DIR` are read in
   `Config::from_env_with_format` (before `blastit_env::build_from_process`
   strips `BLASTIT_*` across the stage-1 `execve`), validated as absolute, and
   carried in the stage-1 document (`libphp`, `php_shim_dir`, and `php` =
   `{libphp, dev, ino, version, shim}` for the `MB.3.13.3` re-open). No child
   environment gains either name: they never leave `Config`.
8. `docs/reference/settings.md` `## Backends`: one table, one row
   (`BLASTIT_LIBPHP` | the distro package's library | names the NTS `libphp` to
   embed for `[route.php]`).

## The exact strings

Missing (no `BLASTIT_LIBPHP`, no candidate) — verbatim on an `x86_64`-gnu host:

```text
route "app": no usable NTS libphp was found for [route.php] entry "public/index.php" (searched BLASTIT_LIBPHP, then libphp8.4.so, libphp-8.4.so, libphp8.3.so, libphp-8.3.so, libphp8.2.so, libphp-8.2.so, libphp.so in /usr/lib/x86_64-linux-gnu, /usr/lib64, /usr/lib, /usr/local/lib/x86_64-linux-gnu, /usr/local/lib); install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
```

Present but unusable — verbatim from this host (PHP 8.4.27, no shim built for
the tuple; the boot prefixes `blastit dev: ` or `blastit check: `):

```text
route "app": no shim libblastit_php84-x86_64-gnu.so was found for the NTS libphp /usr/lib/libphp8.4.so (looked in /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/debug, /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/php-sapi); this host is (PHP 8.4, x86_64-gnu), and libblastit_php84-x86_64-gnu.so is what that tuple needs; install the PHP embed package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed, Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
```

The same shape carries each `present but unusable` problem, always followed by
`this host is (PHP X.Y, <arch>-<libc>), whose embedded shim is <shim>`:

- ZTS: `<path> is a ZTS (thread-safe) libphp, and the embedded worker needs the NTS build of the same version`
- not an ELF: `<path> is not an ELF shared object (<detail>)`
- not a libphp: `<path> exports neither executor_globals nor tsrm_startup, so it is not a PHP embed library`
- not a regular file / ENOENT: `BLASTIT_LIBPHP (<path>) cannot be opened or is not a regular file (<detail>)`
- no version in the name: `<path> is an NTS libphp whose file name names no PHP major.minor, so no shim can be chosen for it; …`

Step 3, unchanged (`MB.3.28`), printed by both boot paths when the host is
usable — verbatim:

```text
route "app": the embedded PHP backend is not available in this build ([route.php] entry "public/index.php"); the per-route worker wiring lands in MB.3.13.3
```

## Which accepted twin runs natively on this host (PHP 8.4.27)

`/usr/lib/libphp8.4.so` is opened, `fstat`ed and parsed as an ELF: `.dynsym` has
`executor_globals` and no `tsrm_startup`, so it is NTS, and its name states
`8.4`. With a shim named `libblastit_php84-x86_64-gnu.so` in a shim directory
the discovery resolves `(PathBuf(/usr/lib/libphp8.4.so), st_dev, st_ino, PHP 8.4,
shim)` — proved against the real distro library, no fixture. In a deployment
scope that pair makes the gate `Proceed`, and the boot then stops on step 3; in
a `dev` boot the same pair stops on step 3 as well. The inline test
`the_host_library_resolves_or_the_refusal_names_the_package` asserts this branch
on a PHP host and the missing-package refusal on a host without one (both are
assertions, neither is a skip); `a_zts_libphp_…`,
`the_shim_is_chosen_by_the_tuple_…` and `a_host_with_no_libphp_…` carry the rest
of the denied/accepted pairs, including the scope twins (deployment refuses a
missing shim, `dev` leaves it to step 3).

## Evidence

`cargo test -p blastit --locked` (acceptance 1):

```text
test result: ok. 178 passed; 0 failed; 1 ignored  (lib)
test result: ok. 4 passed   (critical_php_backend)
test result: ok. 3 passed   (no_python_build)
test result: ok. 19 / 11 / 11 / 4 / 31 passed (critical_check_engine, critical_config_and_presets, critical_static_cli, audit_group_hint, smoke)
```

`make check` (acceptance 2) — exit `0`, both interim suites untouched:

```text
test result: ok. 21 passed; 0 failed; 0 ignored (blastit-view)
clippy      -p blastit{,-view} (standalone)
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
make_exit=0
```

New gate tests (10) and config tests (3):

```text
test stage0::php_boot_gate::a_host_with_no_libphp_refuses_only_when_a_php_route_exists ... ok
test stage0::php_boot_gate::a_zts_libphp_is_refused_naming_the_tuple_and_the_shim ... ok
test stage0::php_boot_gate::a_libphp_that_is_not_an_elf_object_is_refused ... ok
test stage0::php_boot_gate::the_version_comes_from_the_file_name ... ok
test stage0::php_boot_gate::the_shim_is_chosen_by_the_tuple_and_a_missing_one_names_it ... ok
test stage0::php_boot_gate::a_workers_cap_above_the_view_budget_names_both_numbers ... ok
test stage0::php_boot_gate::the_host_library_resolves_or_the_refusal_names_the_package ... ok
test stage0::php_boot_gate::the_gate_reads_the_elf_and_never_loads_it ... ok
test stage0::php_boot_gate::a_version_less_refusal_still_names_the_tuple_with_a_question_mark ... ok
test stage0::php_boot_gate::a_relative_libphp_is_refused_by_the_gate_too ... ok
test config::php_settings::a_version_is_read_from_the_distro_spellings ... ok
test config::php_settings::the_libphp_paths_must_be_absolute ... ok
test config::php_settings::the_resolved_backend_round_trips_and_an_older_document_still_reads ... ok
```

`make guard`, `make critical-guard` (`no change to a protected critical test`),
`make rs-fmt`, `make rs-lint`, `make rs-doc`, `make py-lint`, `make docs-unit`
(83 passed), `make docs-check` (`No issues found`), `make docs-test`
(`57 fences: 28 executed, 29 accounted, 0 failed`), `make plan-validate`,
`scripts/check-portable-elf.sh` (`no RPATH/RUNPATH`) all pass inside that run.

Boot refusals from the built binary, in a scratch project
(`/tmp/opencode/mb3132`, `[route.php] entry = "public/index.php"`):

```console
$ BLASTIT_MAX_WORKERS=16 blastit dev            # workers = 20
blastit dev: route "app": [route.php] workers = 20 is above this deployment's per-view budget of 16 (BLASTIT_VIEW_MAX_WORKERS, from BLASTIT_MAX_WORKERS); lower workers to 16 or raise the budget
exit=1                                        # step 1

$ blastit dev                                   # workers = 16, this host, no ADR-named shim
blastit dev: route "app": the embedded PHP backend is not available in this build ([route.php] entry "public/index.php"); the per-route worker wiring lands in MB.3.13.3
exit=1                                        # steps 2 (dev-scope: not fatal) then 3

$ BLASTIT_PHP_SHIM_DIR=/tmp/opencode/mb3132/shims blastit dev
blastit dev: route "app": the embedded PHP backend is not available in this build (…); the per-route worker wiring lands in MB.3.13.3
exit=1                                        # steps 2 (resolved) then 3

$ BLASTIT_LIBPHP=/tmp/opencode/mb3132/libphp8.4.so blastit dev   # a text file
blastit dev: route "app": /tmp/opencode/mb3132/libphp8.4.so is not an ELF shared object (it is shorter than an ELF identification); this host is (PHP 8.4, x86_64-gnu), whose embedded shim is libblastit_php84-x86_64-gnu.so; install the PHP embed package — …; or set BLASTIT_LIBPHP=/path/to/libphp.so
exit=1                                        # step 2 refuses: the library is there and wrong

$ BLASTIT_LIBPHP=libphp.so blastit dev
blastit dev: BLASTIT_LIBPHP: must be an absolute path to a libphp.so
exit=1

$ blastit check --json        # stdout captured separately
exit=2  stdout_bytes=0
blastit check: route "app": the embedded PHP backend is not available in this build ([route.php] entry "public/index.php"); the per-route worker wiring lands in MB.3.13.3
```

A static-only manifest is untouched by the gate: `php_boot` returns
`PhpBoot::Absent` (unit test) and `critical_php_backend.rs`'s own
`a_static_only_manifest_is_not_refused_by_this_gate` still passes end to end.
Production's denied case (a missing library, or a missing shim, is fatal there)
is the `PhpShimScope::Deployment` twin inside the three tests above; `blastit`
itself refuses earlier on privileges on this host, so the CLI transcripts above
are the `dev` path.

## Follow-ups (out of scope)

- `Makefile`'s `php-sapi` target still writes `target/php-sapi/libblastit_php84.so`
  (`Minor::shim_file()`), while `ADR-0010` §4 selects
  `libblastit_php84-<arch>-<libc>.so`. `make php-sapi`'s output therefore never
  satisfies the boot selection; `MB.3.18`/the Makefile owner must rename it.
- `check::execute` runs step 3 only: it has no `Config` at that point and
  `check.rs` is outside `paths`. `php_worker_budget`/`php_boot_gate` are ready
  for it (and `MB.3.13.3` rewrites the call site anyway).
- Production shim provenance (`plain_is_protected`, zero `DT_RPATH`/
  `DT_RUNPATH`, `(st_dev, st_ino)` recorded for the shim too) belongs to
  `MB.3.13.3`/`R7`; this node records the `libphp` inode only, and honours
  `BLASTIT_PHP_SHIM_DIR` in `dev`/`check` only (`R8`).
- `docs/knowledge/LESSONS.md` candidate (out of `paths`): the C shim built by
  `make php-sapi` and the name boot selection looks for are two different
  strings, which reads as "no shim for the tuple" on a host that just built one.
