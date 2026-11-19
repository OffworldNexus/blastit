# Log MB.41.1

## 2026-11-19T12:44 [conductor]

created (task) Rewrite core onboarding spine (zensical.toml, README, Home, Getting started, Blastfile, Local, CI/CD) with Linux + cloud app-server framing & linked Python/PHP/Static tabs

## 2026-11-19T12:44 [lead]

status ready -> active

## 2026-11-19T12:44 [lead]

attempt 1/3 started

## 2026-11-19T12:53 [lead]

base dc808a9d72b932cafa38c8e963e5eb345a4485fe

## 2026-11-19T12:53 [lead]

status active -> review

## 2026-11-19T12:53 [lead]

lead-verified green: make docs-check platform-matrix-check zed-check EXIT=0 on current bytes; pages 126/128/129/127/129/127 lines; 11 linked tab groups each of Python (WSGI)/PHP/Static & SPA

## 2026-11-19T12:56 [lead]

REVIEW FIX (filtered by lead): PHP install tab missing archive download; static tabs use uvx while page says no runtime (unify on standalone archive, uv only for Python); local.md throwaway postgres docker run lacks -e POSTGRES_PASSWORD/USER/DB so DATABASE_URL cannot connect; ci.md PHP job 'check || test $? -eq 2' masks refusals + flaky 'sleep 2/kill %1' + static job uses uvx with no setup-uv; dev --env-file flag does not exist; all golden-path Blastfiles fail the taught 'fmt --check' (expand one-line blocks/grants); banner transcript prints '/*' not '/.*'; ci.md shared prose claims pytest for every runtime; ci.md findings table lost raw_ingress; reference/cli.md anchor blastfile.md#formatting dead (add ## Formatting section). OUT OF SCOPE: docs_fences PAGES/STATEMENT_LISTS/Shape census + inventory.toml = MB.41.3 (must support N documents per page, each with its own Shape).

## 2026-11-19T13:00 [lead]

status review -> done
