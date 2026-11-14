# Log MB.3.20

## 2026-11-14T16:09 [implementer]

created (task) Held docs pass for MB.3.17: php.md fence and fallback note, design/ADR supersession lines, AUDIT note

## 2026-11-14T16:16 [implementer]

OWED BY THIS NODE: delete the STALE row in crates/blastit-manifest/tests/docs_fences.rs — const STALE = [("docs/frameworks/php.md", "fastcgi: unknown key", "MB.3.20")]. MB.3.17 retired [route.fastcgi] but may not edit docs/frameworks/**, so docs_fences accounts for that one page: it asserts the fence fails FOR THAT REASON, and the_stale_accounting_names_only_manifests_that_still_fail goes red the moment the page boots. Rewriting php.md without deleting the entry leaves make check red; deleting the entry before the page boots also leaves it red.
