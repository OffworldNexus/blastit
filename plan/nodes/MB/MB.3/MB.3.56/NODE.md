---
id: MB.3.56
title: "Pin [route.php] entry by (st_dev, st_ino) at boot: the ADR wins over the gap (H-007)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.3.7]
paths:
  - "crates/blastit/src/**"
  - "crates/blastit-view/src/**"
  - "crates/blastit/tests/**"
  - "e2e/rootless/**"
  - "e2e/php/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T10:05"
updated: "2026-11-15T10:05"
---

# Goal

Pin `[route.php] entry` at boot by `(st_dev, st_ino)` — the same mechanism `libphp` and the
shim already use — so the script a route executes cannot be swapped between requests, and
prove the decoy case **inside `manifest_dir`** now fails closed.

# Context

`MB.3.7`'s rootless leg measured the gap: `entry` is re-resolved per request. Swapping
`public/` aside *outside* the manifest dir fails closed (500), but a decoy **inside**
`manifest_dir` answers `200 DECOY-RAN`. ADR-0010 §3 claims the boot-time pin; the code does
not have it for this object.

H-007 was answered by the owner: **the ADR wins, so make the code match it — do not amend
ADR-0010 §3 to describe the gap.** The reasons, for the record: the same `(st_dev, st_ino)`
mechanism already exists for `libphp` and the shim (consistency, not new design); the
provenance gate (`MB.3.40`: `manifest_dir` not writable by a route uid) reduces
exploitability but does not make the ADR true, and this milestone has refused "safe enough
that the documentation can lie" twice already; and a process that can write `manifest_dir`
could otherwise swap the executed script between requests, which is a different trust
boundary from "the operator owns the deploy".

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ make rs-test-php-image
$ make e2e
$ make check
$ uv run --no-project tools/critical_guard.py --base <parent sha> --node MB.3.56
```
- Boot records `entry`'s `(st_dev, st_ino)` beside the pair's, and the worker verifies it
  **before each request** (or fails the route closed with a sentence naming the route, the
  entry and the fix — never a silent re-resolution).
- THE DECOY CASE, measured with the decoy **inside `manifest_dir`**: the request that today
  answers `200 DECOY-RAN` now refuses or `500`s, and the response never contains the decoy's
  marker. Paste both runs (before/after).
- THE ACCEPTED TWIN: a normal request on an untouched tree still serves (the happy path is
  not collateral), and `MB.3.7`'s rootless swap leg still records decoy **0** opens.
- A protected test pins the refusal and reddens when the verification is removed (paste the
  red).
- `make check` exit 0; workspace clippy clean; no new dependency.

# Notes

- **If a descriptor pin is infeasible** — e.g. PHP's `open_basedir`/`SCRIPT_FILENAME`
  semantics make it unrepresentable — STOP and report the exact obstacle with the file and
  line that decides it. Do not land a half-pin; amending the ADR would then be the honest
  fallback, and that is the owner's call, not this node's.
- `MB.3.7`'s other two findings stay as they are: the PHPUnit counts are asserted as the
  page's real sentence, and the `§4.7` copy is parsed from `e730527^` with provenance
  recorded.
- Release-blocking: this precedes `MB.3.18`. The entry pin is a security property; packaging
  is not worth shipping before it.
