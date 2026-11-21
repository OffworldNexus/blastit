---
id: MB.26.36
title: "The unsupported-minor sentence must not fire on a supported library at an unprobed name"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-view-php/src/discovery.rs", "crates/blastit/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

Stop the unsupported-minor refusal from naming a stray 8.2 object when the real fault is a supported
library present at a spelling discovery does not probe. The sentence a user acts on must describe
their situation.

# Context

`MB.3.65` (fa91776) replaced the `php_version` story with one plain sentence, `PHP 8.2 is not
supported — Blastit supports PHP 8.3, 8.4 and 8.5`, reached through
`PhpRefusal::UnsupportedVersion`. Its reviewer found:

> `crates/blastit/src/stage0.rs:1614` `installed_unsupported` can name a stray unsupported `libphp*`
> even when the real fault is a supported library present at an unprobed spelling (e.g. only
> `libphp8.4.so.0`) → record supported minors in the scan and return `Missing` (or name the
> found path).

A host with a real 8.4 at `libphp8.4.so.0` (or any name outside `LIBPHP_PROBES`) must not be told
"PHP 8.2 is not supported": that sends them to reinstall a package they already have.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ cargo test -p blastit --locked --test critical_php_backend
$ make check
```

- A host whose only `libphp*` is a **supported** minor at an unprobed spelling reads `Missing` (or a
  refusal that names the path it found), not `UnsupportedVersion`. A host with a genuine 8.2-only
  library still reads the unsupported-version sentence, unchanged, character for character.
- Both directions are tests: the supported-minor-at-an-odd-name case and the 8.2-only case. Show the
  first going red against `fa91776` (paste the wrong sentence), then green.
- The protected suite's unsupported-version case keeps asserting the sentence.
- `uv run --no-project tools/critical_guard.py --base fa91776 --node MB.3.68`, `make check` exit 0.
- Nothing outside `crates/blastit/src/stage0.rs` and `crates/blastit/tests/**`.

# Notes

- Do not re-open discovery ordering (`MB.3.61`) or add an 8.2 path back.
- Both aarch64 legs stay CI-only.

# Provenance

Re-filed from `MB.3.68` at the `v0.2.0` gate: diagnostic refinement when both a stray 8.2 object and an unprobed supported `libphp` spelling exist. Note `MB.36.5` moved `installed_unsupported` from `stage0.rs` into `crates/blastit-view-php/src/discovery.rs`.
