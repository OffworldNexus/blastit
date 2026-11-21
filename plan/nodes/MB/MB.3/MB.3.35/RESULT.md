# RESULT — MB.3.35 (attempt 3)

**STATUS: done.** One artifact, one name: `make php-sapi` writes the ADR-0010 §4.1
name `libblastit_php<major><minor>-<arch>-<libc>.so` with a matching
`DT_SONAME`, `Minor::shim_file()` returns the same shape from the compile-time
`(target_arch, target_env)`, and a native `blastit dev` boot of a `[route.php]`
manifest resolves `target/php-sapi/libblastit_php84-x86_64-gnu.so` with no
`BLASTIT_PHP_SHIM_DIR`. No alias, no fallback, ADR untouched.

**Attempt 2 changes** (from the review): the Makefile's `<arch>`/`<libc>` now
come from the compiler that builds the shim (`$(PHP_SAPI_CC) -dumpmachine`),
never from a filesystem probe, so a gnu host with a musl loader installed can no
longer produce a `-musl` name no gnu boot selects; `PHP_SAPI_ARCH`/`PHP_SAPI_LIBC`
exist for cross builds and a musl tuple with no musl loader is refused; the same
derivation is mirrored in `run-probe.sh` (the e2e driver now asks that script for
the tuple instead of repeating it); the boot's tuple spelling is pinned by a
`#[cfg]`-gated literal test; both `php_native.rs` hints compute the shim name
from `Minor::shim_file()`; and this file records the `make check` report-only
critical-guard line.

**Attempt 3 changes** (review PASS, three MINORs): the Dockerfile comment and the
CI paragraph name `cc -dumpmachine` instead of `uname -m`; and the unmet
Acceptance clause is closed — `PhpBoot::Proceed` now carries the resolved
`PhpRuntime`, and `run`/`run_dev` emit one boot line naming the shim path and
`<arch>-<libc>`, so `blastit dev` reads differently with and without the built
shim.

## What changed

| File | Change |
|---|---|
| `Makefile` | `php-sapi` derives `<arch>` and `<libc>` from `$(PHP_SAPI_CC) -dumpmachine` (e.g. `x86_64-linux-gnu` → `x86_64`/`gnu`), with `PHP_SAPI_ARCH`/`PHP_SAPI_LIBC` overrides; refuses, one line each, an arch other than `x86_64`/`aarch64`, an undecidable libc, a compiler that does not answer `-dumpmachine`, and a musl tuple with no `/lib/ld-musl-*.so.1` on the host. Writes `libblastit_php<digits>-<arch>-<libc>.so` and passes `-Wl,-soname,<same name>`. Missing-`php-config` skip behaviour unchanged. |
| `crates/blastit-php-sapi/src/toolchain.rs` | `Minor::shim_file()` (and `Toolchain::shim_file()`) now return `String` built from `cfg!(target_arch)`/`cfg!(target_env)` (`target_arch()`/`target_libc()`/`Minor::digits()`), never a runtime `uname`. |
| `crates/blastit-php-sapi/src/lib.rs` | Crate docs spell the ABI name; the doctest asserts the full tuple name host-aware. |
| `crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h` | Header comment names the shim per ADR-0010 §4.1. |
| `crates/blastit-php-sapi/tests/critical_shim_contract.rs` (PROTECTED) | Name assertion only: a literal per supported `(arch, libc)` tuple inside `#[cfg]`s; no other assertion touched. |
| `crates/blastit-php-sapi/tests/php_native.rs` | Usage doc names the tuple as `<arch>-<libc>`; the missing-variable panic interpolates `Minor::Php84.shim_file()`, so the hint is true on aarch64/musl too. |
| `e2e/php_sapi_probe.py` | `shim_name(minor, tuple)`; `image_tuple()` now asks the image for `run-probe.sh --print-tuple` (one derivation, not a second `uname` guess) so `readelf` targets the file the image actually wrote. Probe assertions unchanged. |
| `e2e/php-sapi/run-probe.sh` | Mirrors the Makefile: `<arch>`/`<libc>` from `${PHP_SAPI_CC:-cc} -dumpmachine` with the same overrides and refusals; new `--print-tuple` mode for the e2e driver; keeps both existence checks. |
| `e2e/php-sapi/Dockerfile` | Comment: the readelf example names the new file. |
| `crates/blastit/src/stage0.rs` | The stale "make php-sapi still writes …84.so" comment is gone; `target_arch`/`target_libc` document why the tuple is spelled twice; `the_shim_name_matches_the_adr_literal` pins `shim_name()` to the four ADR literals; `PhpBoot::Proceed(Option<PhpRuntime>)` keeps the resolution, and both boot paths emit one `manifest.warning` line naming the shim path and `<arch>-<libc>` (`php_runtime_line`). |

### Review findings, one by one

- **MAJOR (compiler vs loader):** fixed — the tuple now comes from
  `$(PHP_SAPI_CC) -dumpmachine`, with `PHP_SAPI_ARCH`/`PHP_SAPI_LIBC` overrides
  and one-line refusals; `run-probe.sh` mirrors it; the false "same predicate"
  claim is removed above and the musl/riscv/no-`-dumpmachine` refusals are
  quoted under `make php-sapi`.
- **MINOR (stage0 duplicates the tuple, nothing pins it):** the **literal
  assertion** option, not the `Minor::shim_file()` option. `blastit-php-sapi` is
  an *optional* dependency (`php` feature) while `discover_php`/`shim_name()`
  compile on every build — `cargo check -p blastit --no-default-features
  --locked` is green today, so an unconditional import would break it (fixing
  that needs `crates/blastit/Cargo.toml`, outside `paths`), and
  `stage0::php_boot_gate::the_gate_reads_the_elf_and_never_loads_it` already
  forbids the loader crate's name in this file. The tuple therefore stays
  spelled twice and `stage0::php_boot_gate::the_shim_name_matches_the_adr_literal`
  pins `shim_name()` to the four ADR literals; the php-sapi half of those
  literals is the protected test's `#[cfg]` block.
- **MINOR (hard-coded `-x86_64-gnu` in hints):** fixed — the `php-sapi` usage
  panic interpolates `Minor::Php84.shim_file()` (true on aarch64/musl), and both
  doc blocks spell the placeholder `libblastit_php84-<arch>-<libc>.so` with the
  note that `Minor::Php84.shim_file()` prints the exact name.
- **MINOR (report-only critical-guard line):** recorded in the authorization
  section above, with both runs.

### Review MINORs (attempt 3)

1. **`e2e/php-sapi/Dockerfile:15`** no longer says the shim is named for "the
   image's own `uname -m` and libc": it says "the image's own
   `cc -dumpmachine`, see `run-probe.sh`".
2. **This file's CI paragraph** no longer repeats
   `<minor>-<uname -m>-<gnu|musl>.so`: it says
   `libblastit_php<minor>-<cc -dumpmachine's first field>-<gnu|musl>.so`, which
   is what the image build actually derives.
3. **The NODE Acceptance "the boot log names the shim it selected"** is now met:
   `PhpBoot::Proceed` carries `Option<PhpRuntime>` (kept for `MB.3.13.3`), and
   both `run` and `run_dev` emit one `manifest.warning` line —
   `the [route.php] shim is <path> (PHP 8.4, x86_64-gnu)` — before the interim
   refusal, through the emitter each path already uses (`emitter` in `run`,
   `startup`/descriptor 2 in `dev`, so stdout stays empty until the boot is
   known-good). No new event kind. `blastit dev` on the one-line manifest now
   prints the path with the shim present and not without it (quoted below), and
   `critical_php_backend.rs`'s "no fallback recipe" scan still passes because
   the line names no library and no alternative transport.

### Files the lead widened into `paths`

These were originally outside `paths`; the lead widened
`plan/nodes/MB/MB.3/MB.3.35/NODE.md` to cover them (and
`docs/knowledge/LESSONS.md`), so nothing in this attempt is out of scope:

- `crates/blastit/src/stage0.rs` — the stale "make php-sapi still writes …
  84.so" comment is replaced, and the tuple-spelling pin test was added
  (comment/test only).
- `crates/blastit-view/tests/php_native.rs` — a usage line pointing at a file
  that no longer exists (doc only).
- `docs/research/php-embed-sapi.md` — two recommendation lines (docs only).
- `.github/workflows/php-sapi.yml` — the line-5 header comment now names the
  ADR-0010 §4.1 shim (`libblastit_php<major><minor>-<arch>-<libc>.so`, on this
  host `libblastit_php84-x86_64-gnu.so`); the job's `name:` is deliberately
  unchanged (see Follow-ups). No behaviour change in any of them.
- `docs/knowledge/LESSONS.md` — one gotcha line.

## Protected-critical-test authorization

`make check` runs `critical-guard --base HEAD` **without** a node (it cannot know
which node owns the working tree), so it prints a report-only violation for the
one protected file this node authorizes; it does not fail:

```
$ uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 21 changed file(s)
  M  crates/blastit-php-sapi/tests/critical_shim_contract.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
exit=0
```

With the node named — and against this node's base commit — the same edit is
authorized:

```
$ uv run --no-project tools/critical_guard.py --base 71eed2f --node MB.3.35
critical-guard: 71eed2f..working tree, 21 changed file(s)
node MB.3.35: paths ['Makefile', 'e2e/php_sapi_probe.py', 'e2e/php-sapi/**', 'crates/blastit-php-sapi/src/toolchain.rs', 'crates/blastit-php-sapi/src/lib.rs', 'crates/blastit-php-sapi/tests/critical_shim_contract.rs', 'crates/blastit-php-sapi/tests/php_native.rs', 'crates/blastit-php-sapi/c/include/blastit_php_abi_v1.h', 'crates/blastit/src/stage0.rs', 'crates/blastit-view/tests/php_native.rs', 'docs/research/php-embed-sapi.md', 'docs/knowledge/LESSONS.md', '.github/workflows/php-sapi.yml']
  M  crates/blastit-php-sapi/tests/critical_shim_contract.rs  [authorized by crates/blastit-php-sapi/tests/critical_shim_contract.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.35
exit=0
```

The protected edit is still only the name assertion (a literal per supported
`(arch, libc)` tuple inside `#[cfg]`s); every other assertion in that file is
untouched.

## Acceptance

### `make php-sapi`

```
$ make php-sapi
php-sapi: cc -> /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
php-sapi: skipped 8.3: no php-config for PHP 8.3 (Debian/Ubuntu: apt install php8.3-dev libphp8.3-embed)
php-sapi: skipped 8.2: no php-config for PHP 8.2 (Debian/Ubuntu: apt install php8.2-dev libphp8.2-embed)
php-sapi: 1 shim(s) for x86_64-gnu in /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/php-sapi; skipped: 8.3 8.2
exit=0

$ ls target/php-sapi
libblastit_php84-x86_64-gnu.so

$ readelf -d target/php-sapi/libblastit_php84-x86_64-gnu.so | grep -E 'SONAME|NEEDED|RPATH|RUNPATH'
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000000e (SONAME)             Library soname: [libblastit_php84-x86_64-gnu.so]
```

`DT_SONAME` equals the file name, `NEEDED` is libc only, and there is still no
`DT_RPATH`/`DT_RUNPATH`. **The tuple is the compiler's, not the filesystem's:**

```
$ cc -dumpmachine
x86_64-linux-gnu
```

`<arch>` is that triple's first field, `<libc>` is `musl` iff the triple contains
`musl` and `gnu` iff it contains `gnu`. This is the predicate that matches the
boot's `cfg!(target_env)` on the host that *builds* the shim — the previous
`/lib/ld-musl-*.so.1` probe did **not**: a glibc host with Debian's musl package
installed has that loader while `cc`/`php-config` still build glibc, so the old
rule would have written a `-musl` name the gnu boot never selects (and `dev`
would have stayed silent about it). The same applies to any `PHP_SAPI_CC` cross
build, which is why the compiler is authoritative.

The negative cases, with nothing installed or enabled (fake compilers in
`/tmp/opencode/mb335/` that only answer `-dumpmachine`):

```
$ make php-sapi PHP_SAPI_CC=/tmp/opencode/mb335/fake-musl-cc      # reports x86_64-linux-musl
php-sapi: '/tmp/opencode/mb335/fake-musl-cc' targets musl but no /lib/ld-musl-*.so.1 is installed, so this host could not load the shim; set PHP_SAPI_LIBC=musl only for a cross artifact
make: *** [Makefile:430: php-sapi] Error 1
exit=2

$ make php-sapi PHP_SAPI_CC=/tmp/opencode/mb335/fake-riscv-cc     # reports riscv64-linux-gnu
php-sapi: unsupported architecture 'riscv64' (supported: x86_64, aarch64)
make: *** [Makefile:430: php-sapi] Error 1
exit=2

$ make php-sapi PHP_SAPI_CC=/tmp/opencode/mb335/fake-nodump-cc    # -dumpmachine fails
php-sapi: '/tmp/opencode/mb335/fake-nodump-cc' does not answer -dumpmachine; set PHP_SAPI_ARCH and PHP_SAPI_LIBC
make: *** [Makefile:430: php-sapi] Error 1
exit=2
```

The explicit overrides reach the tuple (run with no minors so no artifact is
written), which is how `MB.3.18` will cross-build:

```
$ make php-sapi PHP_MINORS= PHP_SAPI_LIBC=musl
php-sapi: 0 shim(s) for x86_64-musl in …/target/php-sapi; skipped: none
exit=0

$ make php-sapi PHP_MINORS= PHP_SAPI_ARCH=aarch64 PHP_SAPI_LIBC=musl
php-sapi: 0 shim(s) for aarch64-musl in …/target/php-sapi; skipped: none
exit=0
```

`run-probe.sh` mirrors the rule (same `-dumpmachine`, same overrides, same
refusals) and is the single place the e2e driver asks for the tuple:

```
$ sh e2e/php-sapi/run-probe.sh --print-tuple
x86_64-gnu

$ PHP_SAPI_CC=/tmp/opencode/mb335/fake-musl-cc sh e2e/php-sapi/run-probe.sh --print-tuple
run-probe: '/tmp/opencode/mb335/fake-musl-cc' targets musl but no /lib/ld-musl-*.so.1 is installed
exit=1
```

### Acceptance grep (must exit 1)

```
$ grep -rn 'libblastit_php[0-9][0-9]\.so' --include=* Makefile crates e2e docs | grep -v '\.so\.' ; echo "grep=$?"
grep=1
```

`grep=1` means no match outside the ADR's history. A wider sweep over the whole
checkout (`grep -rn 'libblastit_php8[234]\.so\|libblastit_php8\.4\.so' .`,
excluding `target/`, `.git/` and `plan/`) now leaves exactly one hit: the CI
job's display `name:` (`.github/workflows/php-sapi.yml:37`), which this attempt
must not rename — see Follow-ups.

### CI workflow: behaviour was already correct after the rename

The workflow names no artifact file in any command; both of its shim-dependent
steps go through rules that now derive the name, so nothing else had to change:

```
$ sed -n '62,63p' .github/workflows/php-sapi.yml
      - name: make php-sapi-e2e (Docker: debian:trixie-slim + php8.4-dev + libphp8.4-embed)
        run: make php-sapi-e2e

$ sed -n '49p' e2e/php-sapi/Dockerfile
RUN make php-sapi PHP_MINORS="${PHP_MINOR}" \

$ sed -n '462,463p' Makefile
php-sapi-e2e: ## Docker: build one shim and prove it with the real-load probe (not in `check`)
	$(UV) run --no-project e2e/php_sapi_probe.py $(if $(PHP_MINOR),--minor $(PHP_MINOR))
```

`make php-sapi` inside the image writes
`libblastit_php<minor>-<cc -dumpmachine's first field>-<gnu|musl>.so` (this
node's `Makefile` change), `e2e/php_sapi_probe.py`'s `image_tuple()` asks
`run-probe.sh --print-tuple` for the same string inside the same image, and the
`readelf`/probe steps therefore read the file that was built — proven below by
`make php-sapi-e2e` on this host. The only stale text left in the workflow is
the human-owned status-check context.

### `make check`

```
$ make check > /tmp/opencode/mb335/make-check-5.log 2>&1; echo "make_check_exit=$?"
make_check_exit=0

$ grep -n 'VIOLATION\|report only' /tmp/opencode/mb335/make-check-5.log
5:  M  crates/blastit-php-sapi/tests/critical_shim_contract.rs  [VIOLATION: not covered by any node path]
6:  report only: 1 breaking change(s) would fail an enforced run

$ tail -3 /tmp/opencode/mb335/make-check-5.log
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

That `VIOLATION … report only` pair is expected and non-fatal: `make check`'s
`critical-guard` runs `--base HEAD` with no node, so it cannot see this node's
`paths`; the enforced run with the node authorizes the same edit (see the
authorization section). `make check` exits 0.

Also run directly: `cargo clippy -p blastit-php-sapi --all-targets --locked -- -D
warnings`, the same with `--features probe`, `cargo test -p blastit-php-sapi
--all-targets --locked` (5 + 15 passed, 2 ignored native harnesses), `cargo test
-p blastit --lib php_boot_gate::` (11 passed, including
`the_shim_name_matches_the_adr_literal`), and `cargo fmt --all -- --check`.

`e2e/php_sapi_probe.py` and `e2e/php-sapi/run-probe.sh` are **not** covered by
`make check` (`py-lint` only lints `python tools`), so they were checked
separately:

```
$ uvx --offline ruff check e2e/php_sapi_probe.py        -> All checks passed!
$ uvx --offline ruff format --check e2e/php_sapi_probe.py -> 1 file already formatted
$ sh -n e2e/php-sapi/run-probe.sh                        -> sh_n=0
$ sh e2e/php-sapi/run-probe.sh --print-tuple             -> x86_64-gnu
```

and end to end by `make php-sapi-e2e` below.

### `plan.py validate`

```
$ uv run --no-project tools/plan.py validate
plan OK
exit=0
```

## DX proof on this host (`blastit dev`, no `BLASTIT_PHP_SHIM_DIR`)

Manifest `/tmp/opencode/mb335/blastit.toml`:

```toml
version = 1

[[route]]
name = "app"
path = ['.*']

[route.php]
entry = "public/index.php"
```

The build produced that exact name, and the boot **names the shim it selected**
(the NODE's Acceptance clause) before stopping at `MB.3.28`'s interim refusal —
i.e. not at §4.1's "no shim for the tuple":

```
$ make php-sapi
php-sapi: cc -> /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)

$ env -u BLASTIT_PHP_SHIM_DIR .../target/debug/blastit dev --port 18099   # WITH the shim
23:05:16.301 WARN  manifest.warning the [route.php] shim is /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/php-sapi/libblastit_php84-x86_64-gnu.so (PHP 8.4, x86_64-gnu)
blastit dev: route "app": the embedded PHP backend is not available in this build ([route.php] entry "public/index.php"); the per-route worker wiring lands in MB.3.13.3
exit=1

$ mv target/php-sapi/libblastit_php84-x86_64-gnu.so /tmp && env -u BLASTIT_PHP_SHIM_DIR .../blastit dev --port 18099   # WITHOUT it
blastit dev: route "app": the embedded PHP backend is not available in this build ([route.php] entry "public/index.php"); the per-route worker wiring lands in MB.3.13.3
exit=1
```

The two runs differ: the with-shim boot prints the resolved shim path and this
build's `x86_64-gnu` tuple (`PhpBoot::Proceed(Option<PhpRuntime>)` carries the
resolution from the gate; the line is a `manifest.warning` event on the same
emitter each path already uses — descriptor 2 in `dev`, the boot emitter in
`run`). The gdb run below corroborates the same resolution at the return of
`discover_php`, including the `libphp` inode pin stage 1 will re-check:

```
$ env -u BLASTIT_PHP_SHIM_DIR gdb -batch -q \
    -ex 'break .../crates/blastit/src/stage0.rs:936' -ex run \
    -ex 'x/s shim_path.inner.inner.inner.buf.inner.ptr.pointer' \
    -ex 'x/s path.inner.inner.inner.buf.inner.ptr.pointer' \
    --args .../target/debug/blastit dev --port 18099
Breakpoint 1, blastit::stage0::discover_php (search=0x7ffffffface8) at crates/blastit/src/stage0.rs:936
936	    Ok(PhpRuntime {
0x555556d17df0:	"/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/php-sapi/libblastit_php84-x86_64-gnu.so"
0x555556d107c0:	"/usr/lib/libphp8.4.so"
```

Negative control (the same breakpoint with the ADR-named file moved out of
`target/php-sapi`): the breakpoint is never reached, so the hit above is the real
lookup, not an unconditional path.

## `make php-sapi-e2e` (Docker leg)

Ran on this host (Docker 29.9.3) after the attempt-2 change, exit 0:

```
$ make php-sapi-e2e; echo "e2e_exit=$?"
php-sapi-e2e: docker build --build-arg PHP_MINOR=8.4 -t blastit-php-sapi-e2e:local
php-sapi-e2e: readelf -d libblastit_php84-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.4 abi 1: status 200, cpu_ns 15180134 <= wall_ns 17475630, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.4 abi 1: status 200, cpu_ns 10402070 <= wall_ns 11985598, exit()+fatal contained, next request served
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.4 abi 1: status 200, cpu_ns 16609037 <= wall_ns 20153573, exit()+fatal contained, next request served
e2e_exit=0
```

This is the whole chain in one image: `make php-sapi` inside
`debian:trixie-slim` wrote `libblastit_php84-x86_64-gnu.so` from the *image's*
`cc -dumpmachine`, `run-probe.sh --print-tuple` handed the driver the same
`x86_64-gnu` (the `readelf -d` line above is that name), the probe `dlopen`ed the
real `libphp8.4` then the shim, and each case passed. No probe assertion changed
(body/head/status/`cpu_ns`/reserved-name drops/`exit()`+fatal/seccomp+INI egress
cases all still checked).

## Notes / follow-ups

- **Follow-ups:**
  1. **Stale CI status-check context — a human must change it.**
     `.github/workflows/php-sapi.yml:37` still reads
     `name: build libblastit_php8.4.so and load it`. The workflow file is now in
     this node's `paths`, but this attempt deliberately does **not** rename it:
     the job `name:` is a branch-protection required status-check context, so
     changing it needs the branch-protection setting updated in the same change.
     A human (repo admin) must rename the job to `build the PHP 8.4 shim and load
     it` (or similar) *and* update `develop`'s required check, or merges will
     block. The header comment at line 5 already names the ADR shim.
  2. ~~`dev` never names the shim it resolved~~ — **fixed in attempt 3** (see
     "Review MINORs"): `PhpBoot::Proceed(Option<PhpRuntime>)` carries the
     resolution and both boot paths emit one `manifest.warning` line naming the
     shim path and `<arch>-<libc>`. `check` has no discovery gate (`check.rs`
     calls only `php_backend_unavailable`), so it still reports the interim
     refusal alone; that is `MB.3.13.3`'s surface, not a shim-name problem.
  3. `MB.3.18`: cross builds now have `PHP_SAPI_ARCH`/`PHP_SAPI_LIBC` (and
     `PHP_SAPI_CC=musl-gcc`), but the *loader* refusal is skipped only when
     `PHP_SAPI_LIBC` is set explicitly — a musl artifact built on a glibc host is
     therefore a deliberate, override-confirmed product. `MB.3.18` should also
     record the tuple in `scripts/platform-matrix.json` next to the sha256.
- `docs/knowledge/LESSONS.md`: added one line about the edit-time formatter
  running rustfmt with the default edition (a 2024-edition file must be
  re-`cargo fmt`ed before `make rs-fmt`).
