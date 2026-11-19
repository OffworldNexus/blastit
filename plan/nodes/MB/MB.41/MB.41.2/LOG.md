# Log MB.41.2

## 2026-11-19T12:44 [conductor]

created (task) Align Deployments, Frameworks, Reference, How-it-works, Security & Troubleshooting with multi-language golden paths & cloud app-server framing

## 2026-11-19T13:00 [lead]

status ready -> active

## 2026-11-19T13:00 [lead]

attempt 1/3 started

## 2026-11-19T13:00 [lead]

base 48f2952a47cc4fb90543b18a5c7d0115ef607563

## 2026-11-19T13:07 [unknown]

edited fields

## 2026-11-19T13:07 [lead]

path widened (lead): e2e/static_assets.py + python/tests/runnable_docs/inventory.toml — static page now documents bare 'blastit dev/check', required so the replay driver keeps accepting it

## 2026-11-19T13:07 [lead]

status active -> review

## 2026-11-19T13:07 [lead]

lead gate: docs-check green (No issues found), platform-matrix ok, zed-check OK(59)+44 tests, e2e/static_assets.py --self-test 17/17 PASS, 14 synchronized tab groups site-wide, zero 'uvx blastit' in docs/

## 2026-11-19T13:11 [lead]

REVIEW FIX (filtered): deploy Dockerfile tabs must be complete runnable images per runtime (anything-else stubs lack COPY/CMD/apt-get update; shared --health-cmd uses python -c for PHP/static); compose-traefik one-line orphan tabs above untabbed Python-only Dockerfile; DO PHP/Static mv only the blastit binary, orphaning share/blastit shims (libblastit_php*.so) and bin->share symlink; DO Static curl with no ca-certificates/curl; django.md admin grants lost auth_permission.content_type_id + django_content_type + auth_user first_name/is_staff/is_active; static-and-spa banner says '1 route' while new sentence claims counts none (mount-only site prints 0 routes) -> change page to 0 routes and re-pin e2e/static_assets.py. FILTERED OUT: blastit-init is real (host.rs/caps.rs).

## 2026-11-19T13:14 [lead]

status review -> done
