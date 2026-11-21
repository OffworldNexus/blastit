# Log MB.3.25

## 2026-11-14T17:04 [lead]

created (task) SeccompRole::{View, EmbeddedView} and feature = testutil without widening View

## 2026-11-14T18:31 [lead]

edited fields

## 2026-11-14T18:31 [lead]

PATH REPAIR by lead: added crates/blastit-sys/tests/** so the protected critical_embedded_seccomp.rs named by backend-runtimes.md §7.1 R14 can exist (node acceptance asks for a test that View is unchanged; the design names it in tests/).

## 2026-11-14T18:39 [lead]

status ready -> active

## 2026-11-14T18:39 [lead]

attempt 1/3 started

## 2026-11-14T18:39 [lead]

base e486b54; prereq MB.3.23 reviewed PASS (005fc67 + 59e7999). Brief carries design R14's shared rules (clone3 ENOSYS, clone CLONE_NEW* EPERM, setsid/setpgid EPERM on BOTH roles) plus EmbeddedView's execve/execveat/fork/vfork/non-thread clone, and the protected-test-vs-feature trap.

## 2026-11-14T18:54 [lead]

GOLDFISH REVIEW PASS (fresh eyes). Zero BLOCKER/MAJOR; 5 MINORs being fixed: the zero-widening oracle was self-referential (reviewer mutated a DENIED number and all tests stayed green — now pinned to a literal sorted number list + behavioural ptrace/bpf/mount/unshare probes in the protected file), setsid/setpgid rationale, test SAFETY wording, RESULT counts, and ARCHITECTURE §7.3 logged to MB.3.8.

## 2026-11-14T19:02 [lead]

status active -> done
