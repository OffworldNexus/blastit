# Log MB.3.52

## 2026-11-15T08:01 [lead]

created (task) MB.19 review MINORs: the canonical manifest fence is untested, plus three page corrections

## 2026-11-15T08:01 [lead]

status ready -> active

## 2026-11-15T08:01 [lead]

attempt 1/3 started

## 2026-11-15T08:01 [lead]

base b2d0fa2

## 2026-11-15T08:02 [lead]

status active -> review

## 2026-11-15T08:07 [lead]

REVIEW FAIL (attempt 1). Item 2 (the canonical fence) is accepted — the reviewer reproduced both reds and confirmed the fence is parsed and validated, not counted. Two findings on the django page:
MAJOR: `docs/frameworks/django.md:52-61` — "Django's admin vendor assets reach five segments (`/static/admin/js/vendor/jquery/…`)" is false, and the five globs stop one depth short. Django 6.1.3's own admin ships `admin/js/vendor/select2/i18n/<lang>.js` (e.g. `/static/admin/js/vendor/select2/i18n/de.js`, SIX segments after `/static/`), referenced from `django/contrib/admin/widgets.py:607`, so `collectstatic` fingerprints them; `/static/*/*/*/*/*` cannot match a six-segment path (`static_asset.rs:207-219`: `*` never crosses `/`). Those hashed assets are therefore still revalidated — exactly the failure this item was meant to close. Add `"/static/*/*/*/*/*/*"` and reword to six segments with the select2/i18n example.
MINOR: `docs/frameworks/django.md:45-54` — the `[route.static]` block is an untitled ```toml fragment, so `docs_fences` never parses it and a missing depth in the newly added globs cannot go red. Extend `docs_fences` with a case that prefixes that fragment with a minimal complete manifest and asserts the parsed `immutable` list (including that each glob's depth is what the page claims). If the harness only accepts whole pages, add the fragment as a titled fence on an existing registered page or teach FILES a fragment-with-prefix case — and say which you chose.

## 2026-11-15T08:07 [lead]

status review -> ready

## 2026-11-15T08:07 [lead]

status ready -> active

## 2026-11-15T08:07 [lead]

attempt 2/3 started

## 2026-11-15T08:08 [lead]

status active -> review

## 2026-11-15T08:21 [lead]

status review -> done
