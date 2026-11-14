# Log MB.3.29

## 2026-11-14T19:46 [planner]

created (task) Toolchain::probe must not read a transient fork/exec failure as not-found (flaky critical_shim_contract)

## 2026-11-14T21:46 [lead]

status ready -> active

## 2026-11-14T21:46 [lead]

attempt 1/3 started

## 2026-11-14T21:46 [lead]

base f4bfcbe; REDDENED make check AGAIN during MB.3.33 (rs-test-critical Error 101, an_unsupported_minor_is_refused_by_name expected UnsupportedVersion got NotFound). This is the 5th occurrence across MB.3.23/.25/.13.1/.10/.33. Fix now, not later: FakeConfig's pid+nanos tempdir can collide, and from_exe maps every Command::output() error to NotExecutable which discover_with swallows.

## 2026-11-14T22:12 [lead]

status active -> done
