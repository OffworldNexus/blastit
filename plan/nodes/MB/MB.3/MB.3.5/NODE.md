---
id: MB.3.5
title: "Parse and validate [route.fastcgi] blocks in blastit-manifest"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.4]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/critical_fastcgi_manifest.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Teach `blastit-manifest` the `[route.fastcgi]` grammar exactly as
`docs/design/php-fastcgi.md` pins it — model, parse, boot validation, error messages —
and document the block in `docs/reference/manifest.md`. Additive only: every existing
`version = 1` manifest still parses unchanged, and `[route.static]` + `[route.fastcgi]`
on one route is the supported PHP shape.

# Context

- The design pins the keys, defaults, boot-fatal rules and the static interaction. This
  node implements them; do not add a key the page and design do not show.
- Files: `model.rs` (the policy type and its route exposure), `parse.rs` (keys/shape),
  `validate.rs` (boot rules), `error.rs` (messages that name `file:line`), `lib.rs`
  (exports), `docs/reference/manifest.md` (one reference entry, the same shape as the
  `[route.static]` entry), and a new protected
  `crates/blastit-manifest/tests/critical_fastcgi_manifest.rs`.
- Expected fatal rules (the design is authoritative): missing / absolute / `..`-bearing
  entry script; an entry script that is not a file under the route's static `root` when
  both blocks are declared; a relative, missing, non-socket or wrong-owner pool socket
  (owner check only where the design says, i.e. production/dev as the invoking uid);
  `workers = 0`; unknown key; a `fastcgi` block on a route with a request-body
  `[route.ingress]` rule the design forbids; a `[route.static]` block whose `root`
  cannot contain the entry script.
- Warn-and-run only where the design says so, through the existing `ManifestWarning`
  shape.
- `crates/blastit-manifest/tests/docs_fences.rs` now validates the PHP page too: add
  `docs/frameworks/php.md` to `FILES` and bump the array/comment by one, in the same
  change as the parser — a `[route.fastcgi]` fence cannot pass before the grammar
  exists, and must pass from now on.
- Compatibility suites (`critical_manifest_011.rs`, `critical_manifest_edit.rs`,
  `grants.rs`, `routing.rs`, `uid.rs`, `validate.rs`, `parse.rs`) are **not** in
  `paths` and must stay green untouched.
- No dependency changes: `Cargo.lock` belongs to `MB.3.4`/`MB.3.8`.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_fastcgi_manifest --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked      # every existing suite, untouched
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test docs_fences --locked
$ grep -q 'docs/frameworks/php.md' crates/blastit-manifest/tests/docs_fences.rs && echo "php page is machine-validated"
$ grep -n 'route.fastcgi' docs/reference/manifest.md
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.3.5
$ make check
```

# Notes

- **Protected tests: add, never weaken.** The new critical file proves the negative
  side — one manifest per fatal rule failing with a message naming the offending line —
  and the positive side: the accepted page's PHP manifest parses, validates, and keeps
  `[route.static]` working beside it.
- `docs/reference/manifest.md` also carries `MB.6`'s static-asset wording right now.
  Add the `[route.fastcgi]` entry (keys, defaults, one line on the static interaction)
  and leave every static sentence exactly as it is; never "rewrite while you are here".
- Keep it table-driven and small: budget ~350 changed lines including tests. If the
  grammar needs a second policy type or a conditional default the design did not pin,
  stop and log a follow-up rather than inventing it.
- Do not touch `crates/blastit-scheduler/**` (`MB.3.6`), `crates/blastit/**`
  (`MB.3.6`), `crates/blastit-fastcgi/**` (`MB.3.4`), or `python/**`.
