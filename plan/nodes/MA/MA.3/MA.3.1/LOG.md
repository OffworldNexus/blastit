# Log MA.3.1

## 2026-11-12T19:16 [planner]

created (task) Runnable-docs harness executing README and docs snippets unprivileged

## 2026-11-12T22:58 [lead]

status ready -> active

## 2026-11-12T22:58 [lead]

attempt 1/3 started

## 2026-11-12T22:58 [lead]

base 5596dc29793945570b5836b149c5ba1e43918c37

## 2026-11-12T23:35 [lead]

status active -> review

## 2026-11-12T23:41 [lead]

REVIEW FAIL (1/3): [MAJOR] harness injects BLASTIT_MEM_HIGH_WATERMARK=100 + BLASTIT_MAX_VIEW_PROCESSES=8 (masks 503); [MAJOR] _static_command ignores 'uv run' targets (no existence/flag check); [MAJOR] docs/local.md fence 3 runs a fixture manifest, not the page's queries=[] block; [MINOR] pgstub substitution counted as executed; --port 9000->9001 rewrite; dev --learn drift note unconditional PASS; RESULT drift-1 repro contradiction; online uv pip install fallback + assert_clean digit-erasing. Product gaps moved to MA.3.7.

## 2026-11-12T23:41 [lead]

status review -> ready

## 2026-11-12T23:41 [lead]

attempt 2/3 started

## 2026-11-12T23:41 [lead]

retry base 5596dc29793945570b5836b149c5ba1e43918c37

## 2026-11-13T00:31 [lead]

status ready -> active

## 2026-11-13T00:31 [lead]

attempt 3/3 started

## 2026-11-13T00:31 [lead]

attempt 3: delete the two expect_drift pins (MA.3.7 (b)/(c) landed; assert 42501 and the one-pass query write), per-run UV_CACHE_DIR so a rebuilt wheel cannot hide behind the global cache, then docs-test must be green unknobbed

## 2026-11-13T00:38 [lead]

status active -> review

## 2026-11-13T00:51 [lead]

status review -> done
