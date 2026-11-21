---
title: Security review — dev fanotify live reload (pass 2)
lens: security
target: docs/design/dev-fanotify-reload.md (revised, status In review)
adr: docs/decisions/ADR-0014-dev-fanotify-live-reload.md
status: final
---

# Security review — `dev-fanotify-live-reload` (pass 2)

Scope: the revised design, ADR-0014, the research note, the four accepted DX pages,
`VISION.md`, `ARCHITECTURE.md` §3/§4/§5/§7.3 and `LESSONS.md`, judged as a fresh
context. The pass-1 security findings F1–F8 are genuinely fixed **in the text** and are not
re-raised: `F_DUPFD_CLOEXEC` poll clone (R2, §4.1, T14), `ReportTable::{register,revoke}`
(§4.4, T15), the hook/report-fd gate (§4.1 step 3, S3, T16/T17), the mid-run
`Effect::Stopped` latch (§4.7, T8), the forged-report bound (R14, S1, §4.4), the
non-blocking report socket, the adoption `learn` keys and the rewritten §4.8 policy
sentence. The new material reviewed hardest: the dev-only dbproxy policy socket, the
adoption document, the `/proc/self/cmdline` re-exec, the `--learn` skip.

## Findings

### F1 · MAJOR (docs/design/dev-fanotify-reload.md:513-520, 696-705)

> one length-prefixed JSON document (`u32 len | bytes`, ≤ 1 MiB) over the dev-only policy
> socket `--dev-policy-fd`, a `socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC)` …

> dbproxy swaps the grants for **new** connections atomically; a document it cannot parse is
> refused whole and the old policy stays (fail-closed).

This is one synchronous write of up to 1 MiB into an `AF_UNIX` **stream** by the supervisor —
the process that reaps children, answers `ctl.chan` `SPAWN`/`RETIRE` and runs the poll loop
that owns the watcher, and the one that is single-threaded whenever it forks. The revision
fixed exactly this class on the *report* socket (`SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK`,
"`EAGAIN` drops the batch and sets `resync`", §4.1 and §4.10) but the *policy* socket has no
`SOCK_NONBLOCK`, no write deadline and no §4.10 row; a document of ~200 KB already exceeds the
default socket buffer, so the write blocks until dbproxy's reader task drains it and stays
blocked if that task is ever stalled. `docs/knowledge/LESSONS.md:11` is the same defect with a
different name (`obs_fd`: "an orchestrator … that stops draining … ends up blocking").

There is also no acknowledgement, so a document dbproxy refuses (over 1 MiB, or a field a
later version rejects) produces no `manifest.warning`: the supervisor retires every view and
emits `dev.reload` as if the allowlist moved, while the old policy stays. Fail-closed, but
silent — `docs/local.md`'s "paste the normalised `query` … and save" fix then fails with the
same `42501` and no explanation, and S7's "refreshed on every `Blastfile` save" is unenforced.

Suggestion: `SOCK_NONBLOCK`, an explicit `EAGAIN` disposition that keeps the **previous
topology** (one warning, no retire, no exec — the same validate-before-teardown rule as step
1), a bounded write deadline, and a one-byte/status ack read before step 3. Add the §4.10 row
and a T18 case where the write cannot complete and the save is refused rather than
half-applied.

### F2 · MAJOR (docs/design/dev-fanotify-reload.md:553-561, 625, 804, 807, 928-931)

> `"learn":{"obs":12,"ctl":13},` / `"policy":14}` … `policy` is present exactly when reload
> is on (it is the stage-1 end of the dbproxy policy socket).

> Production never creates the policy socket and never reads it; the handler is idle without
> the fd.

The cross-exec half of the policy fd **is** specified (it is in the adoption document, so the
`FD_CLOEXEC` clearing at §4.6 step 6 covers it). The ownership half is not. The revision adds
four long-lived descriptors to the process that forks every view — the policy write end, the
adopted `learn` obs/ctl fds, and a permanent duplicate of the scheduler's `http.chan` end —
and states an explicit "no view's `keep` list names it" rule plus a critical test only for the
two fds it did **not** add (the group fd, R14/T14; the report socket, S3/T14/T17). For
`policy`, the sole gate is "the handler is idle without the fd": a document is honoured from
whatever fd dbproxy is given, and that document carries `"unrestricted":true` per listener —
the value `docs/local.md` warns "gives away every statement on that store, including the ones
reading password hashes". Nothing says the write end is absent from every view's `keep` list,
nothing makes dbproxy refuse `--dev-policy-fd` unless its boot config says
`"privilege": "unprivileged-dev"`, and no row enumerates it: T14 scans each view's fd table
only for `anon_inode:fanotify`, T16/T17 enumerate the report socket and the loader global, and
MB.23.6's production transcript counts only `fanotify_init`/`fanotify_mark`. One `keep`-list or
zygote regression hands every view `unrestricted` dbproxy access — against a real upstream
with real credentials — and the suite stays green.

Suggestion: one sentence in §4.1 mirroring R14 ("the policy write end, the `learn` obs/ctl fds
and the adopted `http.chan` end are `FD_CLOEXEC` except across the adoption exec, and no view's
`keep` list names them"), dbproxy refusing `--dev-policy-fd` outside
`privilege: unprivileged-dev`, and T14/T17 extended to assert no view's fd table holds any of
the four. Add a production/`check` assertion for the policy fd to MB.23.6's transcript, which
today proves only the fanotify syscalls.

### F3 · MINOR (docs/design/dev-fanotify-reload.md:224-225, 335-351, 466-468)

> "Drain what the kernel has queued; `EAGAIN` is an empty, non-overflow readout."

The only defined non-event on a report end is `EAGAIN`; the closed-peer case is undefined. A
view that closes `blastit_loaded_fd` while still serving (a stray `os.closerange`, a C
extension, or a malicious worker that wants to spin PID 1) leaves its read end registered until
`reap_children`. An `AF_UNIX` datagram end whose peer is gone is reported readable and reads 0
(and any reply to it gets `ECONNREFUSED`), so an `on_fd` that treats "not `EAGAIN`" as "try the
codec again" re-polls a permanently-ready fd and burns the supervisor's loop for as long as the
view lives — a failure that is neither inert nor warned. The receive side's datagram bound is
also unstated: §4.4 bounds what a *sender* builds (64 paths / 32 KiB), not the buffer the
supervisor reads a forged datagram into, nor that a truncated batch is refused whole.

Suggestion: define `read == 0` / `POLLHUP` (and `ECONNREFUSED` on any reply) as `revoke(source)`
with one debug line — not a `manifest.warning`, so the "one warning" contract holds — state the
receive buffer size and that a truncated datagram is refused, and extend T15 (a view closes its
fd while serving; the supervisor idles, and the next save still reloads once).

### F4 · MINOR (docs/design/dev-fanotify-reload.md:516-523, 618-622, 726, 808)

> matched by `(route, proxy)` … a document that adds or removes a listener is applied only for
> the listeners that exist

The semantics for a **boot listener with no entry in the document** are not stated, and the
safe reading is not the only one available. Deleting a grant ("It does not [belong there] …
narrow this view's pattern", `local.md`) leaves the socket in place because the listener set
did not change; if dbproxy's rule is "replace the grants of the listeners the document names",
the old allowlist survives and the retired view's replacement connects to the same socket and
keeps the statement the developer just removed. That is fail-open relative to the `Blastfile`,
and S7's claim ("refreshed on every `Blastfile` save before the views retire") is then false for
every *removal*. T18 only tests the widening direction ("makes the denied statement succeed"),
so a removal regression is green.

Suggestion: state in §4.6/§4.8 that the document **is** the whole policy — a boot listener with
no entry is granted nothing (fail closed), exactly like an unparsable document — and add a T18
case that deletes a granted `query` and asserts the next connection gets `42501`.

### F5 · MINOR (docs/design/dev-fanotify-reload.md:526-544, 562-574, 140)

> 5. Stop the scheduler only: `kill(sched_pid, SIGTERM)` … 6. `execve` the **stage-1 program
> prefix this image was started with** …

> An import-time exception after the exec is a boot failure with the boot's own sentence
> (`exit 1`), the same as a cold boot

By the time the image execs, the scheduler is stopped and every view is gone. If `execve` itself
fails (a wheel's venv removed mid-run, a prefix that no longer resolves, `ENOMEM`), or the new
image cannot re-read the `Blastfile` it validated (a second, half-saved write lands in the
window T31 exercises), or `preload`/the app raises at import after a save that passed step 1's
syntax-only check, the outcomes are a half-topology with a live `http` and no scheduler, or
`exit 1` — and `PR_SET_PDEATHSIG(SIGKILL)` on `http` and dbproxy means that exit drops `:8080`
and every accepted connection. §4.7 has no row for any of these, R13's "every failure … serve"
does not hold on this path, and `docs/local.md`'s "keeps `:8080` up" is false for a
`Blastfile`/`preload` edit that passes validation but fails at import. The `/proc/self/cmdline`
prefix check also has no failure disposition (empty prefix, > 8 tokens).

Suggestion: state the disposition explicitly (an honest `exit 1` with the failure sentence is
acceptable for `dev`; "serves without reload" is not), re-check the prefix (`access(X_OK)`,
non-empty, ≤ 8 tokens) **before** step 5 so the cheap failures happen while the old topology
still serves, and either accept and document the import-failure exit or trial-exec in a forked
child that reports readiness before the old image replaces itself.

## Resolution

Re-checked all five findings against the current `docs/design/dev-fanotify-reload.md` and the
two crate functions the fix rests on. All five are now genuinely addressed.

| # | Reported | Verified in the current text | Real? |
|---|---|---|---|
| S-F1 | blocking, unacknowledged policy write | §4.6 step 2: `SOCK_STREAM \| SOCK_CLOEXEC \| SOCK_NONBLOCK`; the document is "written through the poll set **until complete** (a short write on `O_NONBLOCK` waits for writability, still inside the 100 ms deadline)", then the ack is awaited through the poll set (`policy_ack`, 100 ms); timeout/`0x01` keeps the old policy + one `manifest.warning`; §4.10 "policy document" row. | yes |
| S-F2 | policy/`learn`/`chan` fd ownership unstated and untested; dbproxy took the fd anywhere | §4.6 step 2: the config gains `policy_fd: Option<i32>` + `dev_policy: bool`, both set only by stage 0's `dev` path; the dbproxy end is `--dbproxy-policy-fd=N`, "accepted only with `--role=dbproxy`; no `--dev-*` token reaches a role other than stage 1", and "a `--dbproxy-policy-fd` without `dev_policy: true` is a configuration error"; §5 S3: no view `keep` list; T14: no view fd table has the policy/adoption/`learn` descriptors. | yes |
| S-F3 | a closed report end had no disposition | §4.4: `POLLHUP`/`POLLERR` ⇒ `revoke` in the same wakeup; T15. | yes |
| S-F4 | a boot listener omitted from the policy document kept its boot allowlist | §4.6 step 2: built from the **new manifest's own `(route, proxy)` pairs** via `route_declares_proxy` and **authoritative** — whole-map replace; emptied grant = present entry with no queries; dropped block = absent pair; both evaluate against `&self.empty`. T18: "replaces the map (so an omitted pair denies) … a removal still denies". See the verification note below. | yes |
| S-F5 | no disposition for a failed `execve` | §4.6 step 6: fatal reload error, print + `_exit(1)`. | yes |

**S-F4 verification.** The premise I rejected last pass is now true in the code the design
names. `route_declares_proxy` (`crates/blastit/src/stage0.rs:3460-3473`) returns true for an
inline empty grant block and for an attached group that names the proxy — pinned by its own
test `an_explicit_empty_grant_still_names_the_proxy_for_its_route` (`stage0.rs:4159-4185`), so
a route that keeps `postgres {}` and empties it produces a **present** entry with no queries
rather than being dropped by `effective_grants`' `!grant.is_empty()` filter
(`crates/blastit-manifest/src/model.rs:1010-1013`). A dropped block produces an absent pair,
and because the document is authoritative the whole map is replaced, so that pair evaluates
against `Learning::policy`'s `unwrap_or(&self.empty)`
(`crates/blastit-dbproxy/src/learn.rs:314-318`) — deny. Both edits fail closed, T18 covers the
removal, and the earlier "keeps its boot allowlist" sentence is gone.

**The two residual `MINOR`s are fixed.** The short write is looped through writability inside
the 100 ms deadline (§4.6 step 2); `Policy::reconcile` is bounded to
`mtime >= since && <= the reconcile instant` (§4.5, §4.6 step 7), so a future mtime cannot
re-arm it, and T31 is differential over the seam.

No new `BLOCKER` or `MAJOR` came with this edit: the authoritative replacement is the
fail-closed direction (`Learning::policy`'s empty allowlist), the new `--dbproxy-policy-fd`
cannot reach a production dbproxy (`dev_policy` is stage-0-authored and required), learn mode
is unaffected because `ListenerMode::Learn` allows-and-records independently of the map, and
the existing-session snapshot still dies with its view.

## Verdict

S-F1–S-F5 are all real in the text and in the cited code; the removals fail closed, the policy
write is bounded and acknowledged, the closed-peer and `execve` failures are dispositioned,
and no new sandbox, secret or privilege path appeared. The remaining items are the ordinary
`MINOR`s of the feature (dev-only, fail-closed), none of which gates acceptance.

VERDICT: PASS
