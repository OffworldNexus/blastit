---
id: MB.3.28
title: "Interim fail-closed refusal: [route.php] in an unwired build exits 1 and never falls through to WSGI"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.13.1]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/check.rs
  - crates/blastit/tests/critical_php_backend.rs
  - crates/blastit/tests/no_python_build.rs
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T19:29"
updated: "2026-11-14T20:43"
---

# Goal

Make the gap between `MB.3.13.1` (a `PhpEmbedRuntime` with no caller) and
`MB.3.13.3` (the supervisor wiring) **fail closed at boot**: a manifest with
`[route.php]` is refused in stage 0 with one actionable sentence, instead of
being accepted and then killing the container (Python-free build) or forking a
WSGI view for the PHP route (Python build). This is the interim guard; `MB.3.13.3`
removes it when `SpawnSpec::Php` is wired.

# Context

- The reviewed state is worst-case: `[route.php]` parses, `can_boot_without_app()`
  says yes, and `Ctl::Spawn` has no PHP arm — so the first request either
  `fail()`s the whole container or runs a WSGI view. A manifest that looks like
  PHP support and silently runs the wrong runtime is remote from its cause.
- Use the **existing capability-gate idiom** (`stage0::no_python_support`,
  `MB.3.11`/`MB.3.16`): a `pub(crate) fn php_backend_unavailable(manifest) ->
  Option<String>` called from the same two places `app_rule`/`no_python_support`
  are called (boot, and `check`), returning a sentence that names the route and
  the fix. One idiom for "not in this build", not two.
- The sentence must name: the route (`route "app"`), `[route.php] entry
  "public/index.php"`, and that **the embedded PHP backend is not available in
  this build** — and that no `[route.fastcgi]`, php-fpm pool or WSGI fallback
  will serve it. It is a build/wiring fact, not a missing-package fact: do not
  tell the operator to `apt install libphp` (that is `MB.3.13.2`'s sentence for a
  host that *does* wire PHP).
- `tests/no_python_build.rs::a_php_only_manifest_is_not_refused_by_the_capability_gate`
  currently asserts the opposite. It is a normal (non-critical) test and it must
  flip to the new sentence in this change, or `rs-no-python` goes red. The
  *capability* gate's own function still returns `None` for a PHP route (it is
  about Python); only the boot-level outcome changes.

# Acceptance

```console
$ cargo test -p blastit --test critical_php_backend --locked
$ cargo test -p blastit --locked
$ cargo test -p blastit --no-default-features --features php --test no_python_build --locked
$ make check
```
- **Denied case (protected, `crates/blastit/tests/critical_php_backend.rs`):**
  a `[route.php] entry = "public/index.php"` manifest under `blastit dev` exits
  `1`, writes **nothing on stdout**, and stderr contains `route "app"`,
  `[route.php] entry "public/index.php"` and `embedded PHP backend is not
  available in this build`. It must **never fork a WSGI view**: assert the
  absence of the WSGI startup surface (the `blastit dev · single-uid simulation`
  banner and any `view ...` line), not just the exit code. Hold the port open so
  a boot that got past the gate cannot bind it and cannot hang the suite.
- **Accepted twin:** a static-only manifest is not refused by this gate (the
  sentence is absent; the boot proceeds to the bind, which the held port makes
  fail for a reason the test controls). A `[route.static]` + `[route.php]`
  manifest (the documented page shape) is refused by the same sentence.
- **Wiring check:** comment the call site and the protected suite goes red — the
  assertion is on the sentence, and `not_python_build`'s WSGI/retired-key cases
  keep their own sentences.
- `make check` and `make test-critical` green; `critical_guard --base <sha>
  --node MB.3.28` clean (this node **adds** the protected file).

# Notes

- Function and its protected suite are **interim**: `MB.3.13.3` wires
  `SpawnSpec::Php`, deletes the refusal, and rewrites the suite to assert a PHP
  route boots and serves `200` through `view.chan` (authorized with
  `critical_guard --node MB.3.13.3`). Say so in a comment beside the function.
- Do not add a `[route.fastcgi]` or WSGI fallback anywhere.
- PR #64 is WIP; this makes the WIP branch honest rather than half-working.
