# Log MA.3.10

## 2026-11-13T01:19 [test-curator]

created (task) Harden static deploy checks, wheel provenance and offline/HOME isolation

## 2026-11-13T02:05 [lead]

status ready -> active

## 2026-11-13T02:05 [lead]

attempt 1/3 started

## 2026-11-13T02:05 [lead]

base 19c37b1d41c4cf5c683a16a44afa559122e46178

## 2026-11-13T02:24 [lead]

status active -> review

## 2026-11-13T02:35 [lead]

REVIEW FAIL (1/3): [MAJOR] harness.py:2454,2460 PSQL.booleans lists -C and -g which psql does not accept, so impossible flags pass the static check; regenerate from psql --help + probe. [MINOR] UV_OFFLINE not actually set in base_env despite docs; documented_snippets misses indented toml blocks; README:13 claims every fence executed; README:43/:58 network/maturin contradictions.

## 2026-11-13T02:35 [lead]

status review -> ready

## 2026-11-13T02:35 [lead]

attempt 2/3 started

## 2026-11-13T02:35 [lead]

retry base 0cb3baff404cf57623015e644c4d2efb2a03fb0c

## 2026-11-13T02:48 [lead]

status ready -> review

## 2026-11-13T02:51 [lead]

status review -> done
