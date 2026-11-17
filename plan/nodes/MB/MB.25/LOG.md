# Log MB.25

## 2026-11-17T01:28 [planner]

created (epic) Clean runtime startup diagnostics: INFO runtime line (PHP version + php.ini, or Python version + virtualenv) instead of WARN manifest.warning

## 2026-11-17T06:20 [lead]

lead: 25.1 done (ee8bfb7). 25.2 must keep the frozen /info/init///// words and the pretty shape printed in docs/local.md; docs_fences.rs and inventory.toml needed no edit.

## 2026-11-17T06:20 [lead]

lead: correction — the frozen words are `runtime` (level info, component init) with `entrypoint` and, per entrypoint, `version` + `libphp` + `php_ini` (php) or `version` + `environment` (python); the pretty shape is the table printed in `docs/local.md`.
