# Log MB.3.57

## 2026-11-15T11:56 [lead]

created (task) Port blastit-sys to aarch64-gnu and x86_64-musl so the 4-leg matrix can exist

## 2026-11-15T11:56 [lead]

status ready -> active

## 2026-11-15T11:56 [lead]

attempt 1/3 started

## 2026-11-15T11:56 [lead]

base a86fd31

## 2026-11-15T12:12 [lead]

status active -> review

## 2026-11-15T12:17 [lead]

Reviewer PASS, five MINORs recorded for the tail (none blocks the port): (1) `sock.rs:37-41` says a saturated `control_len` is "clamped by the caller's buffer bound" but in `collect_fds` a saturated length would be an over-read — unreachable today, so either refuse via Option or reword to "lossless for every Linux field width"; (2) `:453` the no-/proc error is the bare `libc::chmod` errno with no path — wrap it with mode+path; (3) the `ENOSYS`→`chmod_socket_via_proc` DISPATCH arm is executed by no test (qemu honours 452, this kernel is 6.12) — inject the syscall through a test-only seam so a refactor cannot make the fallback unreachable; (4) `:475` "the architectures that share the unified table" lists x86/s390x (which don't) and omits m68k — the real invariant is "452 is fchmodat2 there"; (5) RESULT's cause of the 14 qemu failures is wrong in detail — measured: `seccomp(2)`→ENOSYS, `prctl(PR_SET_SECCOMP)`→EINVAL, `/proc/self/status` shows the emulator's `Threads: 2`, so `install()` fails at the `NotSingleThreaded` guard before any filter is built (not at an AUDIT_ARCH mismatch); the qemu env also needs `-Wl,--sysroot=…` to be reproducible. Conclusion stands (aarch64 seccomp unproven under qemu → CI leg).

## 2026-11-15T12:17 [lead]

status review -> done
