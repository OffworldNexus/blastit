# Log MB.3.11

## 2026-11-14T14:39 [lead]

created (task) Feature-gate pyo3 (feature=python); decouple RawFrame transport and forking from the GIL

## 2026-11-14T14:39 [lead]

status ready -> active

## 2026-11-14T14:39 [lead]

attempt 1/3 started

## 2026-11-14T14:39 [lead]

base 4cd727b; ADR-0010 §4: standalone archive must ship php with python OFF (zero libpython). This leaf unblocks the shim (MB.3.12) and the Python-free binary; it owns Cargo.toml/Cargo.lock for the whole PHP subtree.

## 2026-11-14T14:55 [lead]

status active -> review

## 2026-11-14T14:55 [lead]

attempt 1 report: feature gate + interpreter-free build proven by serving a static 200 from the no-libpython binary; pyo3 tree count 0; one fork call site; make check green; 419+/14- over 10 files + py.rs. Follow-ups to route: MB.3.13 must make blastit-view framing public; critical_backend_capability.rs (design R12) unwritten; check on an all-static python-less manifest not yet refused.

## 2026-11-14T15:01 [unknown]

edited fields

## 2026-11-14T15:01 [lead]

status review -> ready

## 2026-11-14T15:01 [lead]

status ready -> active

## 2026-11-14T15:01 [lead]

REVIEW PASS (attempt 1) with two MINORs to clear before commit: (a) stage0.rs:362 gives a [route.fastcgi]-only manifest advice that cannot work ('install the Python package / add [route.static]') -- branch on fastcgi first and say this build cannot serve external FastCGI yet (MB.3.13), keeping uv add for routes with no executor; (b) the #[cfg(not(feature=python))] wiring is hand-proved only -- no test fails if those call sites vanish, and a crate-wide allow(dead_code) hides the orphan; add a no-default-features refusal test and scope the allow. Paths extended to crates/blastit/tests/** for (b).

## 2026-11-14T15:07 [lead]

status active -> done

## 2026-11-14T15:07 [lead]

commit 90056a7 (make check exit 0, rs-no-python green, 0 pyo3 deps). Both review MINORs cleared: the [route.fastcgi]-only refusal now names MB.3.13's wiring instead of offering uv add/[route.static]; the gate wiring is proved by tests/no_python_build.rs (fails if the call sites vanish) and the crate-wide allow(dead_code) is gone.
