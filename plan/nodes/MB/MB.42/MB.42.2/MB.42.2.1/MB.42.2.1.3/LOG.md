# Log MB.42.2.1.3

## 2026-11-19T17:21 [planner]

created (task) Stage-0 discovery call site + fd handoff + PyForkHooks (no stage-1 dlopen)

## 2026-11-19T18:45 [lead]

status ready -> active

## 2026-11-19T18:45 [lead]

attempt 1/3 started

## 2026-11-19T18:45 [lead]

base d3e87e3171e8bbac52b88c0d9762f3cc03fd2c7e

## 2026-11-19T19:03 [lead]

status active -> review

## 2026-11-19T19:08 [lead]

REVIEW FIX (filtered): BLOCKER PYTHONHOME now names the DISCOVERED prefix while stage 1 still Py_Initialize's the LINKED interpreter and the minor-agreement guard was deleted -> Fatal Python error init_fs_encoding exit 134; BLOCKER refusals double the head ('blastit dev: blastit dev: ...') because sentence and printer both add it, and JSON lane disagrees; BLOCKER 4/9 refusals (Unresolvable/UnreadableMinor/NoLibpython/NoShim) carry no 'blastit {cmd}:' head; MAJOR NoInterpreter swallowed by cfg!(feature=python) so a missing interpreter boots silently with python=None; MAJOR fail-open Config.python=Some with no descriptors; MAJOR a test that cannot fail; MAJOR cfg(test) fn virtual_env scaffolding; MAJOR Provenance gate selected by command:&str (typo disables deployment checks); minors: 2nd pyvenv.cfg reader, host.rs from_utf8_lossy makes strict RPATH refusal unreachable + tag walk order changed, adopt_py_fds compiles in non-python builds, runtime line lost the release, stage-1 refusals name stages, coverage gaps

## 2026-11-19T19:20 [lead]

flake observed (pre-existing, not this slice): blastit-view-php worker::tests::a_runtime_that_retires_ends_the_loop_after_its_done failed once under parallel make fast-check; passes 4/4 isolated and 3/3 full-lib. Record for MB.42 gate.

## 2026-11-19T19:21 [lead]

status review -> done
