# Log MB.46

## 2026-11-20T06:53 [conductor]

created (task) Fix PyPI sdist workspace Cargo.lock build & universal feature set in build-standalone.sh

## 2026-11-20T06:54 [lead]

status ready -> active

## 2026-11-20T06:54 [lead]

attempt 1/3 started

## 2026-11-20T06:54 [lead]

base 0806d229c4b175ead498e348b3eb0aa0633f77c9

## 2026-11-20T07:04 [lead]

edited fields

## 2026-11-20T07:04 [lead]

acceptance literal corrected: normalize() preserves case (sqlpolicy doctest), so 'select 1' -> 'select 1'; added 'SELECT 1  ;' -> 'SELECT 1'. paths extended with scripts/standalone-runtime-check.sh, stale pre-ADR-0017 WSGI-gate assertions there

## 2026-11-20T07:23 [lead]

status active -> done

## 2026-11-20T07:23 [lead]

REVIEW FIX (review-arch+review-ux, both changes-requested): (1) BLOCKER archive README advertised non-existent 'blastit serve' -> 'blastit [--preset NAME]'; (2) MAJOR WSGI claim outran the artifact (staging glob is libblastit_php* only; production resolves libblastit_py3<minor> beside the binary only) -> header/README/echo/workflow comment now say PHP shims only + how to place the CPython shim, shim staging pinned as next node (MB.42.3.3 slice); (3) MINOR maturin include glob is not gitignore-filtered -> exclude crates/**/{target,__pycache__,.venv}/**, *.profraw; (4) MINOR php capability unasserted -> blastit-view-php presence check; (5) MINOR --require-no-python narrower than discovery -> also refuse BLASTIT_PYTHON/VIRTUAL_ENV/BLASTIT_WHEEL_PYTHON/BLASTIT_PY_SHIM_DIR; (6) DX polish (shim plural, libc-aware DT_NEEDED, wheel-first fallback, ## Try it). Rehearsals green: sdist 439 entries + install + normalize; tarball in debian:trixie-slim --require-no-python 3/3; make fast-check/workflow-check/packaging-gates-test.
