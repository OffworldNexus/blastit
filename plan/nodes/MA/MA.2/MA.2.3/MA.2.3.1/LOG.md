# Log MA.2.3.1

## 2026-11-12T13:12 [planner]

created (task) Leaf 8: python ruff hygiene, Makefile gates and fold literal/atomic hardening

## 2026-11-12T15:09 [lead]

status ready -> active

## 2026-11-12T15:09 [lead]

attempt 1/3 started

## 2026-11-12T15:09 [lead]

base aa401fd704bafcaf119c5196bf9bd447b10873f6

## 2026-11-12T15:18 [implementer]

implemented: ruff clean python+tools, Makefile skip guard + offline staging, fold literal refusal + atomic write; RESULT.md written

## 2026-11-12T15:19 [lead]

status active -> review

## 2026-11-12T15:19 [lead]

owner done, sent to review (base aa401fd)

## 2026-11-12T15:22 [lead]

REVIEW FAIL (attempt 1): (1) MAJOR fold._write_atomic follows a planted temp symlink (must use O_CREAT|O_EXCL|O_NOFOLLOW, unlink stale temp, + planted-symlink critical assertion); (2) MINOR multi-statement + unsafe-function refusals not pinned in python/tests/critical; (3) MINOR refusal note echoes the literal value on stderr (redact).

## 2026-11-12T15:22 [lead]

status review -> ready

## 2026-11-12T15:22 [lead]

attempt 2/3 started

## 2026-11-12T15:27 [lead]

status ready -> review

## 2026-11-12T15:27 [lead]

attempt 2 done (symlink-safe O_EXCL/O_NOFOLLOW temp, protected negative cases, stderr literal redaction); sent to review

## 2026-11-12T15:31 [lead]

REVIEW FAIL (attempt 2): MAJOR _redact_literal is not backslash-aware so E'…' literals still leak the raw value on stderr (needs E/U& escape handling + protected E'abc\'hunter2' case); MINOR temp opened 0o666 loses the manifest's 0600 mode (chmod before replace); MINOR 95 rewritten standard-tier test_fold.py cases never executed offline.

## 2026-11-12T15:31 [lead]

status review -> ready

## 2026-11-12T15:31 [lead]

edited fields

## 2026-11-12T15:31 [lead]

status ready -> active

## 2026-11-12T15:31 [lead]

attempt 3/4 started

## 2026-11-12T16:14 [expert]

attempt 3 complete: PG18 scan.l sql_regions shared by fold+logs (all 8 literal forms -> single ellipsis, E-string backslash + trailing placeholder safe), fchmod 0600 preservation in _write_atomic, 28 critical + 117 offline standard fold tests green

## 2026-11-12T16:14 [expert]

attempt 3/4 done: ported PG18 scan.l literal lexer/redactor to fold.py (1:1 parity with blastit-sqlpolicy::redact_literals), preserved 0600 mode in _write_atomic, ran all 28 critical + 117 test_fold.py tests offline, make test-critical and make check green

## 2026-11-12T16:16 [lead]

status active -> review

## 2026-11-12T16:16 [lead]

attempt 3 (expert) done: PG18 lexer parity for all 8 literal forms, 0600 mode preservation, offline test_fold load_tests (117 cases); sent to review

## 2026-11-12T16:21 [lead]

status review -> done

## 2026-11-12T16:21 [lead]

committed d291078
