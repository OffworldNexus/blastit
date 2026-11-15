---
id: MB.3.47
title: "critical_php_embed's CI-coverage note is false since MB.3.39 (protected file, doc-only)"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.13, MB.3.39]
paths: [crates/blastit/tests/critical_php_embed.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T04:14"
updated: "2026-11-15T04:14"
---

# Goal

Make `crates/blastit/tests/critical_php_embed.rs`'s own account of its CI
coverage true again: its module doc still says this suite has **no CI witness**,
which `MB.3.39` made false.

# Context

`docs/testing/AUDIT-MB.3.13-CLOSURE.md` (gap **N2**), `crates/blastit/tests/critical_php_embed.rs`
lines 48-58 at HEAD `1523e50`:

```text
//! # CI coverage (`MB.3.13.4` review, follow-up for the CI owner)
//!
//! `make check` runs on a host without PHP, so every CI run exercises the refusal
//! branch **only**. ...
//! ... `.github/workflows/php-sapi.yml` is the only job that has
//! both ..., but it runs the crate's probe and `make php-sapi-e2e`, never this
//! suite. ... until then the containment leg has no CI witness.
```

Every sentence after the first is now wrong about CI:

- `.github/workflows/php-sapi.yml` (the `shim` job) runs
  `make rs-test-php-image`, which builds these two test binaries on the runner
  and executes them against the image's real NTS `libphp8.4.so` + shim;
- a `guard: the pair legs must serve, not refuse` step fails that job when the
  suites print their no-pair refusal line or when the served evidence
  (`served: HTTP/1.1 200 OK`, `retired pid ... respawned pid ...`) is missing;
- `Makefile`'s `rs-test-php` / `rs-test-php-image` and `PHP_SUITE_ARGS` exist for
  exactly this, and `make rs-test-php` works on a PHP host.

`MB.3.39` left the paragraph with a follow-up ("that file is protected and not in
this node's paths"); `MB.3.37` and `MB.3.43` both owned this file afterwards and
neither fixed it. Why it matters at a gate: the paragraph is where a reviewer
learns which legs CI proves, so today it either invites a duplicate `php-sapi`
step (the thing `MB.3.39` already added) or makes a green `shim` job look like it
proved nothing about the containment legs.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ make critical-skip-check
$ uv run --no-project tools/critical_guard.py --base <base-sha> --node <this id>
```

- The `# CI coverage` section says what is true: `make check` proves the refusal
  branch on a PHP-free host, and `.github/workflows/php-sapi.yml`'s `shim` job
  proves the real-pair legs through `make rs-test-php-image` (plus the guard step
  that fails the job when the pair legs refuse or the evidence goes missing).
  No sentence may claim this suite has no CI witness.
- The section names which legs are host-gated (`a_php_route_is_served_...` in
  the sibling file, `no_libphp_is_mapped_above_the_view_worker`, the three
  containment legs, `a_blocking_script_is_charged_cpu_time_not_wall_time`,
  `a_form_body_reaches_the_front_controller`) and which run everywhere
  (the stub-refusal and probe legs, the provenance legs, the `egress` twin).
- `crates/blastit/tests/critical_php_backend.rs`'s module doc agrees (it is not in
  this node's `paths` only if it says nothing false — check it and report; if it
  needs the same edit, say so in `RESULT.md` and file a follow-up rather than
  editing out of lane).
- The edit is **documentation only**: `git diff --stat` shows `//!` lines and
  nothing else; no assertion, fixture, constant or test is touched; no
  `#[ignore]` or skip construct is added; `critical_skip_check` stays clean.
- `cargo test -p blastit --locked --test critical_php_embed` is still 19 passed
  (on a host with a NTS `libphp`: 19 passed, not 19 refused), and the guard
  authorises the file because this node's `paths` name it and this `# Goal`
  explains why.

# Notes

- This is a protected file: `tools/critical_guard.py` refuses an edit unless the
  node's `paths` cover it (they do) and the reason is in `# Goal` (it is).
- While you are in the file, do **not** fix `Boot::launch`'s free-port hand-off
  (the flake `MB.3.40`'s follow-up 4 records, and the reason `await_boot` cannot
  be snapshot-tested). It is a separate, larger change; cite the follow-up in
  `RESULT.md` if you re-read it.
- Keep the section short: the `dx-first` law applies to test docs too. Three or
  four sentences and the leg names are enough.
