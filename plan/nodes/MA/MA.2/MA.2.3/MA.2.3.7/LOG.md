# Log MA.2.3.7

## 2026-11-12T20:50 [test-curator]

created (task) Re-pin test_pytest_plugin.py against Django 6.1 setup statements and the egress warning

## 2026-11-12T20:50 [lead]

status ready -> active

## 2026-11-12T20:50 [lead]

attempt 1/3 started

## 2026-11-12T20:50 [lead]

base d73ded3043cc9df39d1a482bc6b7c4df449ac2b0

## 2026-11-12T21:09 [lead]

status active -> review

## 2026-11-12T21:19 [lead]

REVIEW (1/3) FAIL verdict driven solely by out-of-scope pre-existing issues: test_live_server.py:191,259 asserts un-redacted SELECT 'denied' (R24 redacts to SQL …) and pytest_plugin.py:1657 misses BLASTIT_MEM_HIGH_WATERMARK (host at 89% -> 503 queue_timeout). Node's own acceptance (39/39 plugin tests, 10/10 check, no weakening) passes. Lead decision: accept node work, open MA.2.3.11 for the two live-server defects.

## 2026-11-12T21:19 [lead]

committed as lead-accepted work (out-of-scope findings moved to MA.2.3.11)

## 2026-11-12T21:19 [lead]

status review -> done
