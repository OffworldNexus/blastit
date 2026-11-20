# Log MB.47.2

## 2026-11-20T07:50 [planner]

created (task) CI py-sapi shims & root provenance, wheel --require-shims, standalone .tar.gz CPython shims

## 2026-11-20T08:15 [lead]

status ready -> active

## 2026-11-20T08:15 [lead]

attempt 1/3 started

## 2026-11-20T08:15 [lead]

base 2c31404ae5e3516bb96e1e6f41c0e5e8b0e93a0f

## 2026-11-20T08:15 [lead]

correction: base is 2c31404627a6f28ca740c949452fc68423a1c169 (the previous line's tail was mistyped)

## 2026-11-20T08:24 [lead]

status active -> review

## 2026-11-20T08:27 [lead]

REVIEW FIX (both lenses, lead-verified): [arch-MAJOR] live.yml:53-57 builds the wheel with no --require-shims and feeds e2e/Dockerfile.prebuilt the WSGI demo -> zero CPython shims -> NoShim; [arch-MAJOR] build-standalone.sh staging trusts the filename only, so a cross/mis-flagged libc stages a mislabeled shim that satisfies count 2 (assert readelf machine + NEEDED libc class); [arch-MINOR] five ci.yml 'make py-sapi' steps can skip a minor and exit 0 -> use python-runtime; [arch-MINOR] build-standalone-in-image.sh hardcodes PY_SAPI_LIBC=gnu under a generic --target; [arch-MINOR] make standalone now fails on a fresh checkout -> depend on py-sapi; [ux-BLOCKER] archive README 'WSGI works out of the box' is false on Ubuntu 24.04/Debian 12/Alpine (3.11/3.12 -> UnsupportedMinor) and without a shared libpython (NoLibpython) -> state the real prerequisite + remedies; [ux-MAJOR] shim_lines fallback has no trailing newline so the CPython list concatenates onto it (reproduced); [ux-MAJOR] hardcoded 3.13/3.14 vs --require-py-shims 1; [ux-MAJOR] Try-it block never exercises the new shim; [ux-MINOR] family labels/<minor> undecodable/contradictory PHP fallback/no forward link/docs-test help text + PY_SAPI_REQUIRE_ALL.

## 2026-11-20T08:34 [lead]

status review -> done

## 2026-11-20T08:34 [lead]

commit 0d3f434
