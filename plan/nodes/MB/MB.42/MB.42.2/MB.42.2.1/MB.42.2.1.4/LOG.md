# Log MB.42.2.1.4

## 2026-11-19T17:21 [planner]

created (task) Stage-1 dlopen + PyRuntime supervisor cut-over + drop pyo3 (portable ELF)

## 2026-11-19T19:21 [lead]

status ready -> active

## 2026-11-19T19:21 [lead]

attempt 1/3 started

## 2026-11-19T19:21 [lead]

base 6b2f21198795c8839a6073e418be8e2b1804959f

## 2026-11-19T19:29 [lead]

run 1 stalled at step cap mid-cutover (tree did not compile). Measured: 6 mechanical errors (E0405 LiveInterpreter, E0432 adopt imports, E0425 no_interpreter/PyForkHooks x2/py) + Cargo.lock stale after py-shim gained blastit-frame. Resuming bounded: wiring + build/clippy/lib-tests + Cargo.toml/build.rs/Makefile + PYTHONHOME retirement + portable-ELF. Deferred to run 2: critical_python_shim.rs, make fast-check, RESULT evidence.

## 2026-11-19T19:44 [lead]

authorized protected-test retarget (lead): crates/blastit-view-python/tests/critical_loaded_files.rs asserts the tracker install site by include_str! of crates/blastit/src/stage1.rs, which ADR-0017 §1.2.1 item 3 moved into blastit-py-shim/src/boot.rs. Retarget the witness, keep the identical invariant (one install in the dev gate before first import/preload; one harvest after warmup before close-and-collect). No deletion, no #[ignore], no weakened assertion.

## 2026-11-19T19:49 [lead]

status active -> review

## 2026-11-19T19:54 [lead]

REVIEW FIX (filtered): BLOCKER no gate builds target/py-sapi (rs-test has no prereq, .github has no step) so 3 protected Python suites are red on a clean checkout - acceptance was green only because the shims sat in this worktree; BLOCKER runtime line printed twice on Embed and stage-1 version is the Py_GetVersion banner; BLOCKER Hosted forks with None under a live interpreter when config.python is unset (PHP-only/static-only on legacy wheel) = bare fork with live CPython; BLOCKER every new dlopen refusal leaks /proc/self/fd/<N> because glibc echoes the path given; BLOCKER stage-0 LoadError refusals carry no 'blastit {cmd}:' head; BLOCKER critical_python_shim passes vacuously (println captured) and all-skip passes; MAJOR py_fds held for process lifetime on non-reload boots; hosted_hooks unnamed interpreter; missing reload leg. CI workflow wiring is MB.42.3 (py-sapi.yml already scoped there); Makefile prereq is here.

## 2026-11-19T20:06 [lead]

status review -> done
