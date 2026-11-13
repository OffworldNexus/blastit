---
id: MB.2.4
title: "Resolve and serve assets safely beneath the root (split: MB.2.4.1 resolve, MB.2.4.2 respond)"
kind: epic
status: done
priority: 3
depends_on: [MB.1, MB.2.3]
paths:
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/static_response.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - "crates/blastit-scheduler/tests/critical_static_assets.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-14T00:32"
---

# Goal

Resolve a decoded request path to a regular file **strictly beneath** a declared static root and build the complete static response — `Content-Type`, `Content-Length`, `ETag`, `Last-Modified`, `Cache-Control`, `304` — with no new process, role, uid or supervisor.

# Context

- New module `crates/blastit-scheduler/src/static_asset.rs`, exported through `lib.rs`; small config fields (size cap, immutable flag) in `crates/blastit-scheduler/src/config.rs`. `rustix` with the `fs` feature is already a dependency: use `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS` (the design pins the exact flags and any kernel fallback). **Never** use `canonicalize()` plus a string prefix check as the security boundary.
- **Refuse, before opening:** NUL and other control bytes, `..` segments after decoding, absolute paths, backslash, over-long paths and over-size files; **refuse at open:** symlinks (including a symlink inside the root pointing outside), directories where a file is required, and sockets/FIFOs/devices. Dotfile denial (`.env`, `.git`) follows the design, including its `/.well-known/` decision.
- **Response:** `HEAD` never reads a body; `GET` streams in bounded chunks through the existing response framing; quoted `ETag` from `(dev, ino, size, mtime)`; `If-None-Match` → `304` with no body; `Cache-Control: public, max-age=31536000, immutable` **only** when the decoded path matches a configured hashed pattern, otherwise `no-cache` + `ETag`. Closed `Content-Type` table from the design; unknown extension is `application/octet-stream`.
- This node exposes a resolver + response builder only; wiring it into the request path is `MB.2.5`. Do not edit `scheduler.rs`, `path.rs`, `route.rs` or `rewrite.rs`.
- Budget: ≤ 5 files, ≤ 600 changed lines including tests; production module stays around 380 lines with the attack matrix table-driven in the tests.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.4
$ make rs-test
```

# Notes

- `critical_static_assets.rs` is a new protected file: one case per row of the design's attack matrix (`..`, `%2e%2e`, `..%2f`, `%2F`, `%5c`, double-encoded, absolute, NUL, symlink escape, FIFO/device, dotfiles, oversize), plus the positive cases (`200` asset, `HEAD` with no body, `304` on `If-None-Match`, immutable vs `no-cache` header). It must run unprivileged: build the tree in a tempdir and create the symlinks there.
- If a kernel lacks `openat2`, the fallback must still be a per-segment `O_NOFOLLOW` `openat` walk — record which path the tests exercised.
- Do not add a dependency and do not edit any `Cargo.toml`; the workspace lockfile belongs to `MB.2.8`.
