---
id: MB.23.5.3
title: "Refresh the dbproxy policies on a reload without dropping :8080, and the PHP live-reload row"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.5.2]
paths:
  - "crates/blastit-dbproxy/src/lib.rs"
  - "crates/blastit-dbproxy/src/config.rs"
  - "crates/blastit-dbproxy/src/learn.rs"
  - "crates/blastit-dbproxy/src/state.rs"
  - "crates/blastit-dbproxy/src/server.rs"
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/php_view.rs
  - crates/blastit/tests/critical_dev_reload.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:37"
updated: "2026-11-17T13:43"
---

# Goal

Land design §8's commit **5c** plus the two rows it unblocks: the dev-only dbproxy policy
channel (stage 0's socketpair, `--dev-policy-fd`/`--dbproxy-policy-fd`, the policy-reader
task, `Learning::replace_route_policies` swapping `Arc<Allowlist>` values, the ack write),
so a `Blastfile` grant edit applies on save **without dropping `:8080`**; the PHP live-reload
row; and the watcher-unavailable row.

# Context

- Read `docs/design/dev-fanotify-reload.md` §4.6 step 2 (the whole policy document and its
  ack), §4.8 ("grants apply on save, sockets do not move") and §8's 5c line. `MB.23.5.2`
  shipped the supervisor loop, the exec and the adoption document; this leaf adds the one
  channel that makes a grant edit apply in the same save.
- The dev policy channel: `socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK)`
  created in stage 0; `--dev-policy-fd=N` for stage 1 at boot, `"policy"` in the reload
  adoption document (present exactly when reload is on), and `--dbproxy-policy-fd=N` for
  dbproxy through the existing `--dbproxy-*` argv family (accepted only with
  `--role=dbproxy`; no `--dev-*` token reaches a role other than stage 1). Neither token
  appears in `--help`.
- The document is `u32 len | bytes`, both little-endian, ≤ 1 MiB:
  `{"version":1,"route_policies":[{…RoutePolicySpec…}]}` with the crate's existing member
  names (`route`, `proxy`, `queries`, `transaction_control`, `unrestricted`, `tables`),
  matched by `(route, proxy)`; `fd`/`uid` are **not** members (the boot's identity wins and
  the socket is never rebound). Built from the **new manifest's own pairs**, one entry per
  `(route, proxy)` the new file declares or grants (`route_declares_proxy`, the predicate
  `dev_view_sockets` uses), and **authoritative**: dbproxy replaces the whole map, so an
  absent pair evaluates against the empty allowlist and denies, and a route that empties its
  grants is present with no queries and allows nothing — a removal can never fail open.
- dbproxy side: `config.rs` gains `policy_fd: Option<i32>` and `dev_policy: bool`, both set
  only by stage 0's `dev` path; a `--dbproxy-policy-fd` without `dev_policy: true` is a
  **configuration error**, so a production dbproxy can never acquire the channel;
  `lib.rs` gains the policy-reader task; `learn.rs` gains
  `Learning::replace_route_policies` (new `Arc<Allowlist>` values behind the existing
  `Learning`) and its `policy()` call sites plus `state.rs`'s route to the new map; the
  ack is **one byte** written from `server.rs` (`0x00` swapped, `0x01` refused). New
  connections see the new grants; a connection opened before the swap keeps the grants it
  was admitted under.
- Supervisor side: write the length-prefixed document without blocking (short writes wait
  for writability inside the deadline), then **wait for the ack through its poll set**
  (`policy_ack` state, 100 ms deadline) and only then send `Ctl::Reload`. A timeout,
  `EAGAIN` past the deadline or `0x01` leaves the old policy in force, emits **one**
  `manifest.warning` ("the new grants were not applied; restart blastit dev") and continues
  with the retirement — a dev save never blocks the supervisor and never fails silently. A
  document that adds or removes a *listener* is applied only for the entries that exist and
  the supervisor emits one `manifest.warning` naming the restart.
- Row 10 (PHP): editing an included `.php` reloads all views together and the next request
  returns the new body; a warm compiled template edit needs no restart (the framework
  revalidates it). The row runs in the PHP-enabled configuration, using the probe the
  existing `critical_php_embed` suite established so a host without `libphp` says so in one
  line instead of passing hollow.
- Row 8 (unavailable): a fake watcher returning `Unavailable{EPERM}` yields exactly one
  `dev.reload.unavailable` and `200`s for the rest of the run.

# Acceptance

```console
$ cargo test -p blastit-dbproxy --locked policy
$ cargo test -p blastit --locked --features python --test critical_dev_reload
$ cargo test -p blastit --locked --features python,php --test critical_dev_reload php
$ cargo clippy -p blastit-dbproxy -p blastit --all-targets --locked --features python -- -D warnings
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
$ make check
$ make docs-test
$ uv run --no-project tools/critical_guard.py --base $(git log --format=%H -1 --grep '^\[MB.23.5.2\]') --node MB.23.5.3
$ git diff --quiet -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
```

1. **The swap (T18 unit half):** `Learning::replace_route_policies` turns a denied statement
   into an allowed one; the map is **replaced** whole, so a pair omitted from the document
   denies (including a route that dropped its `postgres {}` block); an unparsable,
   truncated, oversize (> 1 MiB) or wrong-`version` document is refused **whole** with the
   old policy kept; the ack is exactly one byte, `0x00` on a swap and `0x01` on a refusal.
2. **The document shape:** `version: 1`, `route_policies` with `RoutePolicySpec`'s existing
   member names, one entry per `(route, proxy)` of the new manifest, no `fd`/`uid` member,
   and the pair keyed by `(route, proxy)` — a renamed or added pair is applied for the
   entries that exist.
3. **The configuration members:** `policy_fd`/`dev_policy` are set only on stage 0's `dev`
   path; `--dbproxy-policy-fd` without `dev_policy: true` is refused as a configuration
   error; a production dbproxy boot never creates or reads the channel; neither
   `--dbproxy-policy-fd` nor `--dev-policy-fd` appears in `--help`.
4. **The socketpair:** one `AF_UNIX` `SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK` socketpair
   in stage 0, `--dev-policy-fd` to stage 1, `--dbproxy-policy-fd` to dbproxy, `"policy"` in
   the adoption document exactly when reload is on, and **no view's fd table** holds either
   end (checked the way T14 checks fd ownership).
5. **The 100 ms deadline:** a timeout, `EAGAIN` past the deadline or a `0x01` ack keeps the
   old policy, emits one `manifest.warning` with the restart sentence, and still retires the
   views; the supervisor never blocks on a dev save and never reports success it did not
   get.
6. **A grant edit applies on save:** under CI's `BLASTIT_TEST_PG_URL` job, the
   `docs/local.md` denial edit goes from denied to `200` with one `dev.reload` and no
   process re-forked beyond the views (supervisor, `http` and dbproxy pids unchanged); a
   removal still denies.
7. **A listener-set edit warns and keeps serving:** adding or removing a store emits one
   `manifest.warning` naming the restart and keeps `200`s coming.
8. **Row 10 (PHP):** editing an included `.php` reloads all views together and the next
   request returns the new body; a warm compiled template edit needs no restart. The row is
   `#[cfg(feature = "php")]`, uses the `critical_php_embed` probe (raw `std` probe of
   `libphp` + shim) and on a host without a pair prints the repo's one-line
   `refused (no usable libphp + shim on this host): <stderr>` message so `make rs-test-php`
   fails the job instead of the row passing hollow.
9. **Row 8 (unavailable):** a fake watcher returning `Unavailable{EPERM}` yields exactly one
   `dev.reload.unavailable` and `200`s for the rest of the run, with no second watcher
   constructed and no retry.
10. `make check` and `make docs-test` stay **fully green**, and the critical guard reports no
    modified protected file.

# Notes

- `Cargo.toml` and `Cargo.lock` stay **byte-identical**: no new dependency, no feature, no
  workspace member, no hand-written wire structs.
- No new knob or config key beyond `--no-reload` (shipped); the two `--*-policy-fd` tokens
  are internal plumbing and never reachable from a user's command line help.
- Never a second watcher: row 8 constructs a fake watcher in-process only; the production
  path still builds exactly one group.
- Production and `check` never construct `dev_reload` and never create the policy
  socketpair; the handler is idle without the fd.
- `crates/blastit-sys/src/seccomp.rs` is untouched; no filter gains an `AF_UNIX` rule it did
  not already have.
- No test may assert a per-route or surgical restart: a `Blastfile` grant edit reloads every
  view together, and an "only one view restarted" assertion is a bug.
- **Path gap, decide it before writing code:** `PhpViewSpawn` and `serve_reporting`'s PHP
  call site live in `crates/blastit/src/php_view.rs`, which is in **no** child's `paths`
  (the container's list omits it). Row 10 needs exactly one new field there
  (`loaded: Option<PhpLoadedReport>`) plus replacing the current `None` placeholder. If the
  lead has not extended this node's `paths` before dispatch, stop and report it as a
  one-line follow-up child rather than editing outside `paths` silently.
- Keep the policy document bounded (1 MiB) and the ack one byte; do not add a second
  acknowledgement message or a retry loop.
- `Learning`'s `unwrap_or(&self.empty)` is the deny path that makes an absent pair fail
  closed — do not special-case it.
- Do not touch the supervisor's reload cycle itself (`MB.23.5.2`), `crates/blastit-view/**`
  (`MB.23.4`), or `crates/blastit/src/php_view.rs` without the path decision above.
- `critical_dev_reload.rs` is protected: this leaf may append rows 8 and 10 to it (it is in
  this node's `paths`) but may not weaken, skip or delete an existing row.
- The PHP leg is the only row that needs a host pair; everything else must run unprivileged
  and offline under `make check`, with zero `sleep`-based verdicts.
