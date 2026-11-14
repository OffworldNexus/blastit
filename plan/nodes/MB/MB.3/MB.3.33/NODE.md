---
id: MB.3.33
title: "Static root must never serve .php/.phar source (per-request 404, protected)"
kind: task
status: ready
owner: implementer
priority: 0
depends_on: []
paths:
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/rewrite.rs"
  - "crates/blastit-scheduler/src/static_response.rs"
  - "crates/blastit-scheduler/tests/critical_static_entry.rs"
  - docs/frameworks/php.md
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T21:10"
updated: "2026-11-14T21:10"
---

# Goal

Close the **source-disclosure hole** before a `[route.php]` route can be served:
a static root must never answer a `.php`/`.phar` file as bytes. `GET
/index.php` and `GET /x.phar` from a `[route.static] root` return `404`,
whatever the method, without ever opening the file for reading.

# Context

- `crates/blastit-scheduler/src/{static_asset,rewrite,static_response}.rs` have
  no `.php`/`.phar` gate today: `resolve` classifies a regular file, `decide`
  serves it, and `content_type` maps `.php` to `application/octet-stream`. The
  documented layout (`[route.static] root = "public"` beside `[route.php] entry
  = "public/index.php"`) therefore serves the front controller's **source**.
- `MB.9.2.2`'s design already names the fix (`EntryScript` 404 in `plan`), but
  its dependency chain (`MB.9.2.1.3 ← .1.2 ← .1.1 ← MB.9.1.1 ← MB.9.1.2`) is
  all `ready`; the hole must not wait for that refactor. This node fixes the
  behaviour in the **current** static path; `MB.9.2.2` carries it forward
  unchanged (its brief already forbids serving source).
- Owner decision (2026-11-14), one answer, documented: **per-request `404`,
  not a boot refusal.** The canonical Symfony/Laravel layout puts `entry`
  *under* `root`; a boot refusal on any `.php` under `root` would refuse the
  documented manifest, and an "except `entry`" exception is a second policy.
  So: `root` may contain the entry, and **nothing** `.php`/`.phar` is ever
  served; keep other sources out of `root` anyway.

# Acceptance

```console
$ cargo test -p blastit-scheduler --test critical_static_entry --locked
$ cargo test -p blastit-scheduler --locked
$ make check
```
- `crates/blastit-scheduler/tests/critical_static_entry.rs` (new, protected;
  authorized by `critical_guard --node MB.3.33`) proves, against a tempdir root:
  - `GET /index.php` -> `404`, body empty/not source bytes, and the file was
    never read-opened (assert via a root mode/`O_NOFOLLOW`-style helper or a
    sentinel unreadable file: a `000`-mode `.php` must still 404, not 500);
  - `GET /x.phar` -> `404`; `POST /index.php` -> `404` (not `405`);
  - case variants `.PHP`, `.Php`, `.PhAr` -> `404`;
  - **positive twin**: `GET /assets/app.css` -> `200`, exact body, expected
    `Content-Type`; a dotfile/normal 404 at the same time still behaves.
- The gate runs before `decide`'s `405`/`413` (a body or a wrong method on a
  `.php` path is still `404`), and no `.php`/`.phar` descriptor is ever opened
  `O_RDONLY`.
- `docs/frameworks/php.md` and `docs/reference/manifest.md` regain the one
  true sentence: "`.php`/`.phar` files under a static root are refused with
  `404`; only `entry` is executed." (The prior guarantee was removed because
  the code did not implement it; now it does.)
- `MB.3.8`'s release checklist carries, verbatim: "no `.php`/`.phar` under a
  static root is servable (`404`, protected `critical_static_entry`), proven
  before the 0.2.0 tag."
- `make check` green; `critical_guard --base <sha> --node MB.3.33` clean (adds
  the protected file).

# Notes

- No behaviour change for non-PHP files; no new dependency; no config knob.
- `MB.9.2.2` must keep this gate in its `plan` and keep the protected suite (or
  move it to the conformance suite with the same cases).
