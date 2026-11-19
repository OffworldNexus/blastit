# Log MB.42.2.1.2

## 2026-11-19T17:21 [planner]

created (task) blastit-py-shim cdylib (9 C exports) + make py-sapi for CPython 3.13/3.14

## 2026-11-19T18:01 [lead]

status ready -> active

## 2026-11-19T18:01 [lead]

attempt 1/3 started

## 2026-11-19T18:01 [lead]

base b3c23ef6740d36765ad88e5b533a2ba7be8c8095

## 2026-11-19T18:13 [lead]

status active -> review

## 2026-11-19T18:16 [lead]

REVIEW FIX (filtered): BLOCKER serve_view returns 1 on success vs blastit-view-python zygote.rs child_exit 0 (clean shutdown looks like a crash); MAJOR report_fd write-end leaks into every forked view when LoadedTracker install fails; MAJOR PyBufV1 ownership undocumented and store() frees before write (UB on unzeroed slot); MAJOR artifact invariant unproven (RTLD_LAZY + --unresolved-symbols=ignore-all; only Py_InitializeEx import + SONAME asserted; no py-sapi.yml, no py_shims matrix legs); minors: zygote_state aligns/unsafe, abi_v1 not wrapped, null out succeeds silently, check_syntax ignores zygote, Makefile --locked/RUSTFLAGS/env hygiene, check-portable-elf shim message

## 2026-11-19T18:25 [lead]

status review -> done

## 2026-11-19T18:25 [lead]

commit dcb870f1601fd30d05673025412985fa4783fa47; fast-check green, critical_guard clean, guard OK; py-sapi asserts T blastit_py_abi_version/py_minor/abi_v1 via nm -D --defined-only
