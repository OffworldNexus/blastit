# MB.42.10 — `blastit` compiles for musl again (`libc::execveat` is gnu-only)

`crates/blastit` links on `x86_64-unknown-linux-musl` again, so both musl
archives, both `py3-none-musllinux_1_2` wheels and every musl worker suite are
back. The handoff still starts the **verified descriptor** with
`AT_EMPTY_PATH`; only the way the kernel is asked changed.

## What changed

1. **`crates/blastit/src/cli/handoff.rs` (`exec_verified`, ELF branch)** —
   `libc::execveat(fd, "", argv, envp, AT_EMPTY_PATH)` became
   `libc::syscall(libc::SYS_execveat, fd, "", argv, envp, AT_EMPTY_PATH)`.
   - Why: the wrapper is declared **only** for glibc
     (`libc-0.2.190/src/unix/linux_like/linux/gnu/mod.rs:1205`); the musl module
     has no such function, so `cargo build -p blastit --target *-musl` failed to
     compile (four artifacts plus every musl suite leg).
   - The syscall **number** is declared for both libcs
     (`…/musl/b64/x86_64/mod.rs:490` → 322, `…/musl/b64/aarch64/mod.rs:566` →
     281), and `AT_EMPTY_PATH` is the **shared** `…/linux_like/mod.rs:1218`
     `0x1000` (`include/uapi/linux/fcntl.h`) and not a per-libc constant — so
     `libc::AT_EMPTY_PATH` is used as-is and **no local `0x1000` was added**.
   - **One code path for both libcs. No `cfg(target_env = "musl")` anywhere.**
   - Arguments are widened to `c_long`: `syscall` is C-variadic and the libc
     reads each one back as `long`, so a `c_int` argument would leave the upper
     half of the register unspecified.
   - Security properties are unchanged and stated in the same comment: the exec
     is `execveat(bin_fd, "", …, AT_EMPTY_PATH)` on the descriptor `fstat`ed by
     `verify()`, never a re-opened path; the `#!` branch keeps `parent_fd` /
     `bin_fd` alive across its `execve(path)` (the `Verified` borrow); one
     `unsafe` per branch with its SAFETY note.
2. **`crates/blastit/tests/critical_handoff.rs`** —
   `the_exec_never_reopens_the_verified_path` now asserts on the **syscall
   number** (`libc::SYS_execveat` once), that the call goes through
   `libc::syscall(`, that `AT_EMPTY_PATH` is present, that there are exactly two
   `unsafe` blocks (one per branch), and keeps the existing
   `verified.bin.as_raw_fd()` + no-`canonicalize`/`open(`/`openat(`/`metadata(`
   assertions. Nothing was weakened: it is the same `AT_EMPTY_PATH`-on-the-
   verified-fd promise, asserted on the portable form.
3. **`scripts/platform_matrix.py`** — `("8.6", "musl")` is `suites: True` with
   `suites_because` deleted (the reason is gone), its comment updated, and
   `SHIM_REACHES[("8.6","musl","x86_64")]` moved `probed` → `served` with the
   measured evidence below.

## Evidence (container-only; nothing installed on this host)

musl toolchain image: `blastit-php-sapi-e2e:mb4210-86-musl`
(`alpine:edge` + `edge/testing`, rustc 1.100.0, host triple
`x86_64-unknown-linux-musl`), built by
`make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=musl PHP_SAPI_IMAGE=blastit-php-sapi-e2e:mb4210-86-musl PHP_SAPI_KEEP=1`
— i.e. with `PHP_SAPI_SUITES=1`, the build the old `suites: False` skipped.

### 1. x86_64 musl links, and the artifact is portable

`RUSTFLAGS="-C target-feature=-crt-static"`, same triple (no `--target`, the
repo's own musl recipe in `scripts/build-standalone.sh:93-100`: passing
`--target` for the host triple compiles build scripts **without** RUSTFLAGS and
a static musl build script cannot `dlopen` libclang for `pg_query`'s bindgen):

```
--- [1] cargo build --release --locked -p blastit --bin blastit --no-default-features --features php,static
    Finished `release` profile [optimized] target(s) in 4m 11s
  Type:   DYN (Position-Independent Executable file)
  Machine: Advanced Micro Devices X86-64
 (NEEDED) Shared library: [libgcc_s.so.1]
 (NEEDED) Shared library: [libc.musl-x86_64.so.1]
--- scripts/check-portable-elf.sh target/release/blastit
ok   target/release/blastit: NEEDED libgcc_s.so.1 libc.musl-x86_64.so.1; no RPATH/RUNPATH
PORTABLE-ELF: PASS (exit 0)
--- [2] cargo build -p blastit --locked   (default features, same triple)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.33s
ok   target/debug/blastit: NEEDED libgcc_s.so.1 libc.musl-x86_64.so.1; no RPATH/RUNPATH
PORTABLE-ELF(debug, default features): PASS
```

(The debug build is the one the image's suite step produced at 23:30; it
re-used it, which also means the exact binary that ran the suites below is a
linked `x86_64-unknown-linux-musl` `blastit`.)

### 2. aarch64-unknown-linux-musl

No aarch64 musl **linker** is reachable here: Alpine ships only bare-metal
cross tools (`gcc-aarch64-none-elf`, `binutils-aarch64-none-elf`), and the host
has no binfmt/qemu (`/proc/sys/fs/binfmt_misc/` is empty;
`docker run --platform linux/arm64 alpine:3.21 uname -m` → `exec format error`,
and `tonistiigi/binfmt --install arm64` loads no working handler). A cross
`cargo check -p blastit --target aarch64-unknown-linux-musl` also stops earlier
than our code, in `ring`'s build script, for the same missing C cross compiler.

What *is* proven is the changed expression itself, compiled for that target:

```
targets: aarch64-unknown-linux-musl x86_64-unknown-linux-musl
  check x86_64-unknown-linux-musl    ->     Finished `dev` profile ... in 1.86s
  check aarch64-unknown-linux-musl   ->     Finished `dev` profile ... in 1.05s
  build x86_64-unknown-linux-musl    ->     Finished `dev` profile ... in 1.26s
```

The scratch crate holds the exact call from `handoff.rs`
(`libc::syscall(libc::SYS_execveat, fd as c_long, c"".as_ptr(), argv, envp,
libc::AT_EMPTY_PATH as c_long)`), so `SYS_execveat`, `AT_EMPTY_PATH` and the
C-variadic form are all present on aarch64 musl. The real aarch64 archive is
built by the arm64 runner in CI (`LEGS` keeps `aarch64-musl` enabled).

### 3. gnu unchanged, and the descriptor proof still runs at runtime

```
cargo test -p blastit --locked --test critical_handoff
running 15 tests
test an_elf_candidate_runs_from_the_verified_descriptor ... ok
test the_exec_never_reopens_the_verified_path ... ok
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
```

`an_elf_candidate_runs_from_the_verified_descriptor` really execs a copied
`/bin/true` through the new `syscall` form, so the glibc runtime path is proven,
not just compiled.

```
cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.42s
env -u PYO3_PYTHON cargo clippy -p blastit --all-targets --locked --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.40s
```

### 4. The musl PHP worker suites run again (`make rs-test-php-image`)

`make rs-test-php-image PHP_SAPI_IMAGE=blastit-php-sapi-e2e:mb4210-86-musl
PHP_SAPI_IMAGE_MINOR=8.6 PHP_SAPI_IMAGE_LIBPHP=/usr/lib/php86/libphp.so`:

```
running 9 tests
symlinked /tmp/blastit-php-backend-7-symlink/libphp8.so -> /usr/lib/php86/libphp.so  served: HTTP/1.1 200 OK
served: HTTP/1.1 200 OK  cpu_ns=17118759  wall_ns=45040123  (libphp /usr/lib/php86/libphp.so)
test result: ok. 9 passed; 0 failed; ...
running 29 tests
test result: ok. 29 passed; 0 failed; ...
running 8 tests
served: 2 file(s) after the clean request, 2 after the bailout: [...]
test result: ok. 8 passed; 0 failed; ...
docker run --user 1000:1000 ...  # the PR_SET_DUMPABLE(0) witness
init pid 8: no libphp/shim; above the view 0 map(s) content-checked, 2 denied (EACCES), 0 gone, of 3 children; euid 1000
test result: ok. 1 passed; 0 failed; ...
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
```

and the matrix now agrees:

```
uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — ... reaches: served 8 of 16 shims, 0 probed, 0 refused, 8 unbuilt
| PHP 8.6, musl | yes |
```

### 5. `make fast-check`

See the tail below (recorded from the gate run after `RESULT.md` was written).

## Follow-ups

- The `aarch64-unknown-linux-musl` **build** is still only proven by the arm64
  CI runner: no aarch64 musl linker and no arm64 emulation exist on this host,
  and `ring`'s build script needs the C cross compiler before `blastit` is
  reached. Not a code gap — the changed expression type-checks for the target.
- Any future descriptor-based exec should use `libc::syscall(SYS_*)` whenever
  `libc` ships the wrapper on one libc only; the module doc comment now says so
  at the call site.
