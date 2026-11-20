# Log MB.44

## 2026-11-20T06:23 [conductor]

created (task) Sync CHANGELOG.md (0.2.0) and RELEASING.md for universal binary, PHP 8.6, Cloudsmith/Forgejo repos, py3-none wheels & Composer wrapper

## 2026-11-20T06:23 [lead]

status ready -> active

## 2026-11-20T06:23 [lead]

attempt 1/3 started

## 2026-11-20T06:23 [lead]

base d6710816c274efc3f5d78f4f9353706bb711cc67

## 2026-11-20T06:26 [lead]

REVIEW FIX: (1) RELEASING wheel line must not claim PHP shims travel in the wheel — publish-pypi.yml wheels_* pass require-shims=py and stage no shim-* artifacts (filed as a new MB node); (2) CHANGELOG PHP package hints must stay per-minor (libphp{dotted}-embed / php{digits}-embed); (3) smoke-tested on 3.12/3.13/3.14 is gnu-only, musl smokes 3.13; (4) handoff: $VIRTUAL_ENV replaces ./.venv, it is an or; (5) 12 container legs are per-format, not 4 packages x 12.

## 2026-11-20T06:27 [lead]

status active -> done

## 2026-11-20T06:27 [lead]

commit bb74e50
