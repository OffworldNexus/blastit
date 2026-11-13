---
id: MB.2.4.2
title: "Build static responses: ETag/304, cache headers, Content-Type table, bounded body - design MB.2.4b"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.2.4.1, MB.2.5.1]
paths:
  - "crates/blastit-scheduler/src/static_response.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/critical_static_assets.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T20:52"
updated: "2026-11-13T20:52"
---

# Goal

Turn an already-resolved `StaticFile` into the complete static response — `Content-Type`, `Content-Length`, `ETag`, `Last-Modified`, `Cache-Control`, `nosniff`, `304`, `HEAD` — and prove the whole §5.2 matrix over both backends. This is the design's `MB.2.4b` (response construction), not a "write the tests later" task: the suite lands with the code.

# Context

- `docs/design/static-and-spa.md` §4.5 in full: the header table (`ETag` = quoted strong `"<size:x>-<mtime_ns:x>"`, no inode/device disclosure; `Last-Modified` = IMF-fixdate; `Content-Length` from the read-open `st_size`, absent on `304`; `nosniff` on every `200`/`304`; never `Date`/`Server`/`Accept-Ranges`/`Location`), the **cache rule keyed on `(answer kind, answering URL path)`** (year only for `AnswerKind::Exact` matching an `immutable` glob; every `index` is `no-cache`; `no-store` on refusals belongs to the scheduler in `MB.2.5.2`, not here), the conditional matrix (`If-None-Match` incl. `W/` and `*`; `Range`/`If-Modified-Since`/… ignored), the closed `Content-Type` table (`.PNG` lowercases first, unknown = `application/octet-stream`), `http_date`'s three vectors, the `immutable` glob vectors; §4.2's `StaticResponse`/`StaticBody` (`respond` consumes the `StaticFile` and moves the `OwnedFd` into `StaticBody` — never `dup`ed); §4.6 the `STATIC_CHUNK_BYTES` bound and "no truncated body under a promised `Content-Length`"; §5.2 the full 36-row matrix; §7.2 (`R5`, `R7`, `R9`, `R10`, `R11` → `critical_static_assets.rs`).
- New `crates/blastit-scheduler/src/static_response.rs` (declared `pub mod static_response;` in `lib.rs`), holding `impl StaticRoot { respond }` plus `http_date`, the `Content-Type` table and the cache rule. `MB.2.4.1` made `StaticRoot`'s fields `pub(crate)` for exactly this. Splitting the responder out keeps each production module under the design's 500-line rule (§8) while §4.2's public surface is unchanged.
- Uses `StaticOutcome`/`StaticFile`/`StaticMeta`/`AnswerKind`/`ImmutableGlob` from `static_asset.rs` (`MB.2.4.1`) and, for the matrix's statuses, `rewrite::decide` (`MB.2.5.1`) — assert statuses through `decide`, never with a second local mapping of §5.2.
- Out of scope: `static_asset.rs`, `config.rs` (`MB.2.4.1`); `rewrite.rs`/`route.rs` (`MB.2.5.1`); `scheduler.rs`, frames, the body pump, the slot/`503` path (`MB.2.5.2`); no dependency changes.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.4.2
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
```

# Notes

- New protected suite `tests/critical_static_assets.rs` is **the** matrix (design §8: "the matrix is one table in one protected file" — never truncate it). Per §7.2: every row of §5.2 for fixtures S/F/N/D over **both** `Backend::Openat2` and `Backend::Walk`; every positive row also asserts the body's sha256 and the answering file's `ETag`; one exact `ETag` string computed from the fixture's `stat`, stable across two GETs, equal across `/a.js`, `/a.js/`, `/a.js?v=2`, and different after a `utimensat` to a known future mtime; `304` for exact and `W/` matches repeating `ETag`/`Last-Modified`/`Cache-Control`/`nosniff` and no `Content-Length`; `HEAD` has `Content-Length` and no body; a zero-length file is `200` with `Content-Length: 0`; `/assets/*` exact → immutable while `/index.html`/`/docs` → `no-cache` (fixture D, rows 34–35); every `Content-Type` row; the three `http_date` vectors; oversize `404`; the three fixed `permission denied` paths. Row 36 (device node) needs root — it is `MB.2.7`'s, `classify` already covers it in `MB.2.4.1`.
- Budget: ≤ 4 files, ≤ 650 changed lines (`static_response.rs` ≤ 260 production lines; the matrix is one table). `lib.rs` gains one `mod` line; this leaf runs strictly after `MB.2.4.1` and `MB.2.5.1`, so that shared line never races.
