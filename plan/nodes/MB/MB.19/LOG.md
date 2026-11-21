# Log MB.19

## 2026-11-14T19:48 [conductor]

created (task) Restructure /frameworks: one 'adapting your app' page + nine capability slots per framework

## 2026-11-14T19:49 [lead]

edited fields

## 2026-11-14T19:49 [lead]

copied into release/0.2.0 from the owner's release/0.2.1 checkout; PATH REPAIR: added python/tests/runnable_docs/inventory.toml (fences are registered by per-page index, so conforming every page re-registers them). SEQUENCING: after MB.3.10 (php.md once, in final shape); NOT parallel with MB.18 despite disjoint docs dirs, because both edit inventory.toml and both run make docs-test (wheel build) - serialise. Do not touch docs/frameworks/{node,jvm}.md (inherit later) and do not touch docs/deploy/**.

## 2026-11-15T07:26 [lead]

status ready -> active

## 2026-11-15T07:26 [lead]

attempt 1/3 started

## 2026-11-15T07:26 [lead]

base 872f523

## 2026-11-15T07:46 [lead]

status active -> review

## 2026-11-15T07:52 [lead]

REVIEW FAIL (attempt 1), two MAJORs and three MINORs, all on the pages:
MAJOR 1: `docs/frameworks/index.md:103-104` says "a static route needs no process". FALSE: a `[route.static]` route in a manifest with `app` and no `spa` must forward misses, so it IS spawned — `crates/blastit-scheduler/src/route.rs:107-127` (`static_forward = policy && !spa && (has_app || php)`) and `scheduler.rs:886-892`; django.md:45-53 and flask.md:37-39 already document that exact shape. Rewrite: "One route is one uid; a route whose static responder forwards gets a process, and a static route with nothing to forward to (`spa = true`, or no `app`) needs none."
MAJOR 2: acceptance 2 forbids framework-independent sentences on the pages, and three are repeated verbatim: the `check --learn` rule at index.md:94-95 = django.md:109 = flask.md:108; `BLASTIT_MAX_WORKERS` at index.md:104-105, django.md:119-120, flask.md:116-117; the migration rationale at django.md:99-100, flask.md:101-102, php.md:80-81. Keep the rule in slot 8, the sizing in slot 9, the migration rationale in slot 7; on the pages leave only what is framework-specific (e.g. "a missing grant on a matching route is `check`'s to report") plus each framework's own command.
MINOR: index.md:67-69 implies `allow_raw` turns the boot warning off. `ManifestWarning::RawIngress` is unconditional (`validate.rs:812-820`); only the `raw_ingress` FINDING is gated (`audit_v1.rs:1059-1072`). Say: "`raw` forwards the bytes unchanged and the boot warns either way; `allow_raw = true` is what stops `blastit check` reporting `raw_ingress`."
MINOR: php.md:42 "the one privileged step on the host" overstates — production `blastit` also starts as root or with capabilities. Say "the one privileged step for PHP itself".
MINOR: index.md:22 "the same keys" is not true (django adds `preload`, php/static omit `app`) → "the same blocks in the same order"; and django.md has no trailing newline (121 editor lines against a 120 budget) → add the newline and cut one line; same newline fix for static-and-spa.md.

## 2026-11-15T07:52 [lead]

status review -> ready

## 2026-11-15T07:52 [lead]

status ready -> active

## 2026-11-15T07:52 [lead]

attempt 2/3 started

## 2026-11-15T07:55 [lead]

visual-checker PASS on the attempt-1 pages (it ran while attempt 2 was in flight, so it judged the structure, not the corrected sentences): /frameworks/ intro and all four framework pages PASS in light, dark and 390x844 mobile; the nine-heading ladder shows zero drift; the capability matrix and static-options tables render without raw pipes and scroll at 390px; code fences are legible and not clipped; drawer nav works; all three bridged anchors (`django.md#manifest` from two deploy guides, `index.md#bind-parameters-server-side` from troubleshooting) land on slots 1 and 3; every framework page carries its own complete manifest shape; 0 console errors, 0 404s. No BLOCKER/MAJOR/MINOR. Attempt 2 is text-only (removing duplicated passages, one corrected sentence, trailing newlines), so the layout verdict carries; if attempt 2 changes any list or table, re-check that page.

## 2026-11-15T07:55 [lead]

status active -> review

## 2026-11-15T07:59 [lead]

status review -> done
