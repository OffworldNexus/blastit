# Log MB.3.60

## 2026-11-15T18:33 [lead]

created (task) The standalone archive must not need the runner's glibc, and check-exec-env must pull before it reads

## 2026-11-15T18:33 [lead]

Queued behind MB.3.59 (one gate owner at a time). Owner's report requirement: after the fix, report the floor each archive actually needs (readelf --version-info | grep GLIBC_ max) and whether wheel (x86_64-musl-cp314) and standalone (aarch64-musl) come back green.

## 2026-11-15T19:36 [lead]

status ready -> active

## 2026-11-15T19:36 [lead]

attempt 1/3 started

## 2026-11-15T19:36 [lead]

base 755281c

## 2026-11-15T20:14 [lead]

status active -> review

## 2026-11-15T20:22 [lead]

status review -> done
