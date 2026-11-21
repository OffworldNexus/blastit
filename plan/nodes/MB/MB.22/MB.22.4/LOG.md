# Log MB.22.4

## 2026-11-16T09:16 [planner]

created (task) Ship editors/zed and the in-repo tree-sitter-blastfile grammar with corpus/query proofs

## 2026-11-17T05:39 [lead]

status ready -> active

## 2026-11-17T05:39 [lead]

attempt 1/3 started

## 2026-11-17T05:39 [lead]

base 8f538ae

## 2026-11-17T05:46 [implementer]

grammar + corpus + queries + zed extension shipped; dotted_name rule added for functions pg_catalog.unaccent; rev=all-zero placeholder for MB.22.5

## 2026-11-17T05:47 [lead]

status active -> review

## 2026-11-17T05:59 [implementer]

round-2 review fixes: method regex widened to [A-Z][A-Z0-9_-]* (M-SEARCH corpus), (dotted_name) @property capture, complete.Blastfile +1 functions directive; all acceptance re-run green

## 2026-11-17T06:03 [lead]

status review -> done

## 2026-11-17T06:03 [lead]

make check: only docs/reference/cli.md fence 1 (dev-flags) fails, on MB.23.1's --no-reload page before MB.23 lands the flag; every other target green

## 2026-11-17T06:03 [lead]

grammar rev pin target = 053b56e (this commit); commit body's last sentence meant: no root Cargo.toml/Cargo.lock change
