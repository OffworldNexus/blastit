# Log MB.42.7

## 2026-11-19T22:52 [unknown]

created (task) Make blastit-sys seccomp unit-test forked children async-signal-safe (fork-inside-libtest deadlock)

## 2026-11-19T22:58 [lead]

status ready -> active

## 2026-11-19T22:58 [lead]

attempt 1/3 started

## 2026-11-19T22:58 [lead]

base 4dc0c2f569d60195bf6b682b0175294ab4d156f7

## 2026-11-19T23:08 [lead]

authorized protected-test hardening: crates/blastit-sys/tests/critical_embedded_seccomp.rs forks (libc::fork at :244 via try_fork at :163,340,368) and its children call install() which reads /proc/self/status and allocates inside a multi-threaded libtest -> identical deadlock pathology. Child-body rewrite only; every assertion and leg kept.

## 2026-11-19T23:19 [lead]

status active -> done
