---
id: MB.21.4.5.2
title: "Prove the static mount's and the php entrypoint's grammar in the two protected suites"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-manifest/tests/critical_static_manifest.rs"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T16:57"
updated: "2026-11-16T16:57"
---

# Goal

The two protected suites that spell their grammar as per-route tables (`[route.static]`, `[route.php]`)
prove the same refusals on the two `Blastfile` constructs that replaced them: a top-level
`static "<prefix>" { … }` mount, and one `entrypoint php "<path>"` for the whole file.

Every accepted form keeps its denied twin, each family keeps its exact boot message, and the root audit
keeps both of its sides — the suite's subject moves from a route to a mount, its promises do not move at
all.

# Context

- Design §4.3 (`static` is a mount; `entrypoint php` is declared once), §7.2/§9 (`R1`–`R5`, `R13`),
  `ADR-0010` §1, and `ADR-0012` §12.3: `workers` is gone, so writing it is a refusal that names the
  removal and the ADR.
- Measured: `tests/critical_static_manifest.rs` 752 lines / 11 retired fixtures / 4 reader calls;
  `tests/critical_php_manifest.rs` 445 lines / 9 fixtures / 9 reader calls. Both are protected.
- The model still carries `StaticPolicy` and `PhpPolicy`, so nothing about the checks has to be
  reinvented: a mount reaches its policy through `manifest.mounts()[i].policy`, the front controller
  through `manifest.php()`, and `Manifest::check_static_roots` audits mounts. The *location* of a refusal
  moves from `Location::Route` to `Location::Mount`, and its sentence keeps naming the same key.
- Two rules lost their subject and must be re-homed, not dropped:
  - The `spa` key under `[route.static]` is gone (§9.3). Its replacement names a static file used as the
    `fallback`, so keep the both-directions pair: that file is not `index`, a dotfile `fallback` is
    refused, and a dot-directory root stays legal.
  - `PhpPolicy.workers` is always `None` and `workers <n>` is a syntax refusal, so the worker-range family
    becomes one refusal naming the removal plus the assertion that **no** worker count is ever sized from
    a file.
- The static suite's `PAGE` mirrors `docs/frameworks/static-and-spa.md`, which still carries the retired
  spelling until `MB.21.5.1`. Build the equivalent `Blastfile` page in the suite (a comment naming
  `MB.21.5.1` is enough); do not read the page.

# Acceptance

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/critical_static_manifest.rs crates/blastit-manifest/tests/critical_php_manifest.rs
$ grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/critical_static_manifest.rs crates/blastit-manifest/tests/critical_php_manifest.rs
$ cargo test -p blastit-manifest --test critical_static_manifest --test critical_php_manifest
$ make critical-guard BASE=<sha before this node> NODE=MB.21.4.5.2
```

1. Both greps print nothing and both suites pass.
2. `critical-guard` is clean: no assertion was deleted, skipped or weakened.
3. `RESULT.md` carries one line per family — `root`, `index`, `immutable`, `fallback`, `declarations`;
  `entry`, `duplicate`, `workers`, `can_boot_without_app`, `is_all_static` — naming the assertion that now
  carries it and confirming the accepted + denied pair survived. A family that lost its denied half must
  say which suite took it.

# Notes

- `StaticPolicy` / `PhpPolicy` stay in the model. Do not remove a model field from a test leaf: the
  per-route `static_policy` / `php` accessors are a recorded follow-up, and `docs_fences`' retired-page
  predicate still reads one of them until `MB.21.5.1`.
- Assert on `manifest.mounts()` and `manifest.php()`, never on `route.static_policy()` / `route.php()`:
  the suite must survive the follow-up that deletes the per-route pair untouched.
- The static page's four-step precedence (a file on disk, then a view, then `fallback`, then `404`) is
  asserted end to end in `critical_blastfile_parity.rs`; this suite keeps the per-key grammar (root,
  index, fallback, immutable) and the declarations a mount that never reaches the app may not make. Do not
  duplicate the precedence walk, and do not lose a key rule.
