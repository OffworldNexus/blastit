# ADR-0014: Live reload in `blastit dev` — one unprivileged file-handle group, exact loaded-file tracking, one all-views reload

- **Status:** Accepted
- **Date:** 2026-11-17
- **Node:** MB.23
- **Owner:** designer
- **Analysis:** `docs/design/dev-fanotify-reload.md` (the full design, **In review** while
  this record is Accepted: the decision is taken, the design that implements it is under
  the four goldfish lenses), `docs/local.md`, `docs/reference/cli.md`,
  `docs/reference/log-events.md`, `docs/troubleshooting.md` (the accepted DX),
  `docs/research/2026-11-16-dev-fanotify-reload.md` (measured ground truth).

## Context

`blastit dev` kept the old advice "the file is read once, restart after an edit", which is
the slowest loop a developer has. The accepted DX promises that a save reloads every view
together, keeps `:8080` up, and needs one off switch (`--no-reload`) — with no `sudo`, no
capability, no new config key and no new `Blastfile` field. Saving a shared module must
never leave two routes on different versions, and an editable package or template loaded
from outside the working directory must be caught too.

Only an unprivileged file-handle group is viable: a normal user may create one since Linux
5.13, must identify objects by handle, may mark inodes only, and always reads `pid == 0` for
events it did not cause, so reload decisions must be path-based. The attribute that makes
it exact is the workers' own reports: the `preload` phase plus every Python and PHP view
worker say which files they actually loaded.

## Decision

1. **One watcher, one group, one owner.** `crates/blastit-sys/src/fanotify.rs` wraps
   `fanotify_init(0x0c03, O_RDONLY)`, inode marks with
   `FAN_CLOSE_WRITE|FAN_MOVED_TO|FAN_MOVED_FROM|FAN_CREATE|FAN_DELETE|FAN_ONDIR|FAN_EVENT_ON_CHILD`,
   a 16 KiB aligned read buffer, and a `(fsid [u32;2], handle_type i32, handle bytes)` table
   fed by `name_to_handle_at(AT_HANDLE_FID)`. One live group per image, held by the
   supervisor, never by a view. `open_by_handle_at` is never called, so no capability is
   needed to resolve a path.
2. **The watched set is the union of loaded-file reports.** The `preload` phase and every
   view worker report over a dev-only `AF_UNIX SOCK_DGRAM` socket carried in the worker's
   `ViewConfig` (an inherited fd; no environment variable, no `BLASTIT_*`); the supervisor's
   union of those absolute paths, plus `./Blastfile`, is the only thing watched. Parent
   directories are marked; the skip list (`.git`, `node_modules`, `.venv`, `__pycache__`, …)
   and framework cache dirs are marked but never trigger.
3. **One reload unit: every view together.** Any tracked change fires one `Ctl::Reload`
   (`0x45`, empty payload) after 50 ms quiet, at most 250 ms after the first event, and
   never less than 500 ms after the previous reload. The scheduler retires every active
   view whose in-flight count is zero with reason `reload` and marks the rest stale; no new
   request reaches a stale view, and a request already in flight finishes. No route
   payload, no dependency map, no single-view restart.
4. **The listener and the proxies survive; a grant edit applies.** `http` keeps the
   listening socket for the whole run: it is never closed, re-bound or moved, so keep-alive
   connections survive a reload. A `Blastfile`, `preload` or boot-imported file save validates first, emits one
   `dev.reload`, refreshes dbproxy's allowlist over a dev-only policy socket (so the
   `postgres { query … }` denial fix applies without a restart), retires every view, stops
   only the scheduler, and re-execs stage 1 with the live `http`, the scheduler's
   `http.chan` end, dbproxy, the `--learn` observation/attribution descriptors, the dev policy socket and the
   runtime root adopted. The preload cycle therefore always runs on a fresh interpreter
   image; a change to the proxy *listener set* still needs a restart and says so.
5. **Two events, one word.** `dev.reload` (`info`: `routes` serving after the reload,
   `files` absolute, `reason` ∈ `loaded|preload|blastfile|overflow`) and
   `dev.reload.unavailable` (`warn`: symbolic `errno`, filesystem type), plus `reload` in
   `process.retired`'s reasons — exactly the vocabulary `MB.23.1` froze.
6. **Every failure serves.** `ENOSYS`, `EPERM`, `EINVAL`, `EOPNOTSUPP`, `ENOSPC`, `EMFILE`
   each produce one `dev.reload.unavailable` (the human line keeps `reload disabled` and
   the errno) and the server keeps serving. `ENOSPC` stops marking and keeps what was
   marked. There is no retry loop, no second mechanism and no crash.
7. **Dev only, no new privileges.** The module is constructed only on the `dev` supervisor
   path; `--no-reload` yields zero fds, marks and hooks; production, `check` and `test`
   never create a group; `crates/blastit-sys/src/seccomp.rs` is byte-identical and no
   filter is widened. No new dependency: `Cargo.toml` and `Cargo.lock` do not change.
8. **Two test seams and no more.** `trait ReloadWatcher { mark_dir; read }` with one real
   implementation and one fake, and an injected `Clock`, so debounce and the floor are
   asserted at exact instants with zero `sleep`.

**Kernel and filesystem floor.** The unprivileged group needs Linux **5.13**;
`AT_HANDLE_FID` needs **6.5**. On 5.13–6.4 the watcher probes with `flags = 0`, which
works on xfs/ext4/btrfs/tmpfs but not overlayfs. When neither works — a kernel without
`CONFIG_FANOTIFY`, a container profile that denies `fanotify_init`, or a filesystem with no
file handles (9p, some FUSE) — `blastit dev` prints the one warning and serves without live
reload. It never refuses to boot and never silently degrades to a second watcher.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| `inotify` | Banned by the owner: one watcher, no second one, not behind a flag and not as a fallback. |
| A per-route dependency map with selective single-view restarts | Two views can serve different versions of a shared module, and `preload` files belong to no route. |
| The non-FID group, or `FAN_MARK_MOUNT`/`FAN_MARK_FILESYSTEM` | `EPERM` for an unprivileged caller; no fallback exists. |
| Replacing PHP's plain file wrapper to observe `fopen` | `php_resolve_path` compares wrapper identity and `open_basedir` lives inside the plain opener; `EG(included_files)` already covers executed files. |
| A stat/mtime polling mode behind an environment variable | A second watcher and a second failure surface; not in `0.2.0`. Deferred, not shipped. |
| Re-execing the stage-0 argv on a `Blastfile` change | After the stage-0 barrier the process no longer holds the store secrets, so stage 0 cannot rebuild dbproxy; the reload adopts it and refreshes only its allowlist over the dev policy socket. |
| Re-forking `http` and handing it the listening fd | The socket would survive but every accepted keep-alive connection would reset. |

## Consequences

- A view save costs one retire-and-fork per view, from the warm immortal parent: a fork,
  never a re-import. A `Blastfile`/`preload` save re-imports once, in the fresh stage-1
  image, behind the same listener.
- Editing a view, a shared module, an editable package or a template reloads every view
  together and `:8080` keeps its connections; editing `./Blastfile` or a `preload` module
  re-runs the cycle behind the same listener, and an intermediate invalid `Blastfile` keeps
  the previous topology serving.
- A `query` grant edit applies on save; the one manifest change that waits for a restart is
  a change to the proxy listener set, and it says so.
- The reload policy (`crates/blastit/src/dev_reload.rs`) is the security-relevant half and
  clears the 75 % mutation floor; the symbol guard makes the ban mechanical on the
  standalone binary and the wheel extension.
