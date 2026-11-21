# Log MB.25

## 2026-11-17T01:28 [planner]

created (epic) Clean runtime startup diagnostics: INFO runtime line (PHP version + php.ini, or Python version + virtualenv) instead of WARN manifest.warning

## 2026-11-17T06:20 [lead]

lead: 25.1 done (ee8bfb7). 25.2 must keep the frozen /info/init///// words and the pretty shape printed in docs/local.md; docs_fences.rs and inventory.toml needed no edit.

## 2026-11-17T06:20 [lead]

lead: correction — the frozen words are `runtime` (level info, component init) with `entrypoint` and, per entrypoint, `version` + `libphp` + `php_ini` (php) or `version` + `environment` (python); the pretty shape is the table printed in `docs/local.md`.

## 2026-11-17T22:11 [lead]

gate passed: MB.25.1 (docs) and MB.25.2 (the event) both done; blastit-events 97/5/1, php-sapi 5/8/15/1, critical_php_backend 9, critical_php_embed 27 (x4 incl. under load), docs pytest 35, make check EXIT=0 180 ok with the widened hang leg running twice, Cargo.toml/lock/seccomp unchanged. Three real boot tails recorded.

## 2026-11-17T22:11 [lead]

status ready -> done
