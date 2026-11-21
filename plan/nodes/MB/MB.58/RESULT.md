# MB.58 — Alpine `libgcc` and stage-1 shutdown reaping

## What changed

- `crates/blastit-sys/src/signal.rs`: `reap()` is the only `waitpid` sweep in the
  workspace and now reports exhaustion — `Reaped { ended, exhausted }` — because
  `pid == 0` and `ECHILD` are the two facts a shutdown drain needs and the old
  `Vec` collapsed them. Re-exported from `blastit_sys` (`Reaped`).
- `crates/blastit/src/supervise.rs`: the hand-rolled `unsafe libc::waitpid(-1,
  WNOHANG)` loop in `reap_every_child` is gone. `reap_children` returns
  `blastit_sys::reap`'s exhaustion flag and the drain is `loop {
  reap_children(); if exhausted || deadline.expired() { return } sleep(2ms) }`.
  `handle_reaped` still reports each pid exactly once, exit `0` on `SIGTERM` and
  exit `1` on component death still hold, and `TERMINATE_GRACE` still bounds the
  drain.
- `crates/blastit/tests/common/proc.rs` (previous round): `starttime`/`in_proc`
  read field 3 first and treat `Z` as gone, so a zombie cannot look alive.
- `docs/install.sh`:
  - `ensure_libgcc`'s `step` now names the command it runs (`installing libgcc
    with apk add --no-cache libgcc`), like every sibling `step`.
  - Its failure sentence no longer tells a root user to use `sudo` (that code is
    only reachable as root): `check the network or the apk repository, then
    re-run this installer`.
  - The `--user` refusal is gated on `command -v apk` first and on `libc = musl`
    second, so an apk-less musl host (Void, Chimera, busybox) keeps the generic
    sentence instead of being told to run a command it does not have.
- `e2e/run_packaging.py` `leg_alpine`: prerequisites are split (`apk add
  --no-cache curl ca-certificates` only — `libgcc` is what the installer is
  supposed to install); a real-archive run adds the root path (`sh
  /src/docs/install.sh` as uid 0, no `--user`, no `--prefix`, so `ensure_libgcc`
  runs and the install lands in the container's `/usr/local`) asserting the
  output names `apk add --no-cache libgcc` and
  `/usr/local/bin/blastit --version` succeeds; and the `--user` hint
  (`apk del libgcc`, re-run into `/work/hint-prefix`, expect non-zero whose text
  contains `run: apk add libgcc`, then `apk add --no-cache libgcc` back). Both
  assertion blocks are gated on `real` — a stand-in archive "runs" without
  `libgcc_s.so.1`, so it cannot prove either branch — and neither prints `SKIP`.

## Evidence

```
$ cargo fmt --all && make fast-check
run 21 tests ... test result: ok. 21 passed; 0 failed
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.0 (fixture))
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

```
$ make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1 && make rs-test-php-image
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.67s
rs-test-php-image: root run served, crashed and recovered, no leg skipped
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.64s
rs-test-php-image: unprivileged run served, crashed and recovered, no leg skipped
```

```
$ uv run --no-project e2e/run_packaging.py --require-legs --leg alpine
  note alpine: the archive holds a stand-in (no musl build on this host); build one with
  `scripts/build-standalone.sh --target x86_64-unknown-linux-musl` to run the real
  fmt/check and the root//--user libgcc assertions there
PASS  alpine                alpine:3.21 non-root (fixture archive)

1 passed, 0 failed, 0 skipped (a SKIP names the artifact it needs)
```

**The two new `leg_alpine` assertion blocks did not execute in that run**, because
they are `real`-only and this host has no musl artifact: no musl linker or
`x86_64-linux-musl-gcc`, no archive in `dist/`/`target/packages/`, and
`.../releases/download/v0.2.0/blastit-x86_64-unknown-linux-musl.tar.gz` is a
`404` (the release does not exist yet). They execute unchanged under `--musl-archive
<real tarball>` / `--musl-binary`; nothing about them is skipped on a runner
that has one.

The three `docs/install.sh` branches were therefore proved directly in a throwaway
`alpine:3.21`, with a genuine dynamic musl probe (`NEEDED libgcc_s.so.1`,
`libc.musl-x86_64.so.1`) so the missing library fails exactly like a musl
`blastit` does:

```
$ docker run --rm --network host -v "$PWD":/src:ro alpine:3.21 sh /probe/probe.sh
0x0000000000000001 (NEEDED)  Shared library: [libgcc_s.so.1]
--- libgcc_s.so.1 present? no
=== root path (no --user, no --prefix) ===
root install exit=0
blastit: installing libgcc with apk add --no-cache libgcc
(1/1) Installing libgcc (14.2.0-r4)
blastit: installing into /usr/local
blastit: installed: blastit 0.2.0 in /usr/local
root binary: blastit 0.2.0 (exit 0)
=== --user refusal with no libgcc_s.so.1 ===
libgcc_s.so.1 present? no
hint install exit=1 (want non-zero)
blastit: error: /tmp/hint-prefix/bin/blastit did not run: this musl host has no
libgcc_s.so.1 — run: apk add libgcc, then re-run this installer
=== musl host with no apk: generic refusal ===
generic install exit=1 (want non-zero)
blastit: error: /tmp/generic-prefix/bin/blastit did not run: remove it and re-run
this installer, or report the failure at https://github.com/OffworldNexus/blastit/issues
```

The root-failure sentence (an `apk` that fails, as a root install sees it):

```
$ docker run --rm -v "$PWD":/src:ro alpine:3.21 sh -c '
    printf "#!/bin/sh\nexit 1\n" >/tmp/apk; chmod +x /tmp/apk; PATH=/tmp:/usr/bin:/bin sh /src/docs/install.sh'
blastit: installing libgcc with apk add --no-cache libgcc
blastit: error: blastit needs libgcc_s.so.1 here and 'apk add --no-cache libgcc'
failed: check the network or the apk repository, then re-run this installer
root install exit=1
```

## Follow-ups

- This host has no musl cross compiler (`x86_64-linux-musl-gcc`) and no musl
  release artifact, so the Alpine leg ran with the fixture archive and its
  `real`-only assertions (new root/hint checks, plus `fmt`/`check`) stayed
  dormant; a musl build would exercise them unchanged.
- `fail()` and the fatal `component_exit` arm still `_exit(1)` without the final
  sweep: they already exit non-zero, so a zombie there cannot fake a clean run.
- `docs/install.md`'s Alpine prerequisite wording was left alone (outside this
  node's `paths`).
