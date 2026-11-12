# Log MA.1

## 2026-11-12T00:50 [planner]

created (task) Write lean Local/CI/Deploy documentation & 0.1.1 DX design spec first

## 2026-11-12T01:41 [planner]

edited fields

## 2026-11-12T01:59 [planner]

edited fields

## 2026-11-12T01:59 [planner]

rewrote body: user-facing docs only (Local/CI-CD/Deploy DX); design doc + ADRs now exclusively MA.5

## 2026-11-12T02:05 [lead]

status ready -> active

## 2026-11-12T02:05 [lead]

attempt 1/3 started

## 2026-11-12T02:05 [lead]

base 4ee2982

## 2026-11-12T10:19 [doc-writer]

docs pass 2: seccomp fail-closed + preset refusal, body-cap arithmetic, ingress 413/raw_ingress/allow_raw contract, egress truth, dev --host DEV_EXPOSE, stage-0 barrier wording, global-key parse error, preload/cwd minors

## 2026-11-12T10:22 [doc-writer]

docs pass 2 (attempt 2): Django admin unrestricted boxed as last resort + session group route-level only (F16/M8), frameworks app split into per-framework fences (M11), sslmode plaintext + ca_cert note (S16), any-else env-file audit; RESULT.md with 57-row review resolution; docs-check green

## 2026-11-12T10:26 [doc-writer]

pass 3: closed B2 (severity, per-proxy counts, unsafe), M5 (proxy_unused), M2 (learn --json/--dry-run), S8 (DO migration-role secret), manifest route-level groups; RESULT rows + Files refreshed; make docs-check green, trivia grep empty

## 2026-11-12T10:33 [lead]

REVIEW FAIL (attempt 1): (1) local.md:122 uses invalid [route.products.db] grant (boot abort) -> [route.db]; (2) ci.md fix.to '[route.catalog.db].queries' is not a valid manifest path; (3) settings.md:40 + log-events.md:3 'stdout is the only output' contradict the --json contract (B1); (4) troubleshooting.md:36 maps suite failure to exit 1, contract says 3 (B3); (5) compose POSTGRES_USER:shop is owner + migrate reuses *app-env (S8); (6) DO web DATABASE_URL still owner (S7); (7) django CSRF_TRUSTED_ORIGINS absent (F15); (8) ci.md missing_ingress fix suggests raw without allow_raw => raw_ingress.

## 2026-11-12T10:33 [lead]

status active -> ready

## 2026-11-12T10:33 [lead]

status ready -> active

## 2026-11-12T10:33 [lead]

attempt 2/3 started

## 2026-11-12T10:36 [doc-writer]

pass 4: fixed reviewer blockers on local.md grant, ci.md --json fix paths + allow_raw, stdout/exit-code contract, non-owner DB roles in both presets, Django CSRF_TRUSTED_ORIGINS, preload build; RESULT rows refreshed

## 2026-11-12T10:43 [lead]

status active -> done
