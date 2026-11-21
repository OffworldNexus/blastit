# Log MB.42.2.0

## 2026-11-19T17:20 [unknown]

created (task) Pin fork/zygote/GIL ownership invariants in universal-distribution §1.2.1 (design addendum)

## 2026-11-19T17:20 [lead]

status ready -> active

## 2026-11-19T17:20 [lead]

attempt 1/3 started

## 2026-11-19T17:20 [lead]

base 5b3ce8df52aed5b4954eefd7b5543e16e4bc6ec6

## 2026-11-19T18:19 [lead]

status active -> review

## 2026-11-19T18:22 [lead]

REVIEW FIX (filtered): BLOCKER §1.2.1 order only valid for Interpreter::Embed — wheel/Hosted has CPython live before stage 1 so fork(http)/fork(scheduler) need hooks (forking.rs refuses NoInterpreter when interpreter_live), and with pyo3 gone hooks exist only behind the shim; MAJOR PyRuntime &'static vtable not constructible from LoadedPy + voids PyForkHooks PhantomData guard; MAJOR zygote ownership/drop unstated (shim Box::into_raw, no free_zygote); MAJOR §1.1/§:79 names SharedObject::path() for refusals but path() IS /proc/self/fd — source() is the real path; minors check_syntax alloc note, 'never calls GIL APIs' wording, Adoption OwnedFd owner, MissingSymbols backticks, {cmd} legend, NoInterpreter/libpython{minor}/identity text, plan bookkeeping in §3/§146, ADR PATH clause

## 2026-11-19T18:45 [lead]

status review -> done
