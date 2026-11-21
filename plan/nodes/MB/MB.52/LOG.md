# Log MB.52

## 2026-11-20T12:24 [conductor]

created (task) Stage CPython shims beside blastit for smoke (root) and move pytest-plugin venv to root-owned /opt/blastit-tests

## 2026-11-20T12:25 [lead]

status ready -> active

## 2026-11-20T12:25 [lead]

attempt 1/3 started

## 2026-11-20T12:25 [lead]

base f8eb15a7ce91b8408ed55fb3cc707c1829ba9906

## 2026-11-20T12:49 [lead]

review-arch VERDICT CLEAN on ci.yml + smoke.rs; fix round 1 (python/tests only): PROTECTED_ROOT probe+ancestor walk, staging leak/assert, README venv recipe

## 2026-11-20T12:49 [lead]

OUT OF SCOPE (python/blastit/** not in paths, needs its own node): pytest_plugin._serveable_project copies a non-traversable $HOME project to /tmp/blastit-live-* for production boots, which provenance must refuse

## 2026-11-20T12:55 [lead]

status active -> done

## 2026-11-20T12:55 [lead]

commit 297061e
