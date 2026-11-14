# ADR-0007: Static Asset Serving and Rewrite Precedence Inside the Scheduler

- **Status:** Proposed
- **Date:** 2026-11-13
- **Node:** MB.2.2
- **Owner:** designer

## Context

`0.2.0` promises built-in static serving and clean URLs for SPAs and front
controllers (`docs/frameworks/static-and-spa.md`): a `[route.static]` block, hashed
assets cached for a year, a deep URL answered with `index.html`, a front controller
that reaches the app with `PATH_INFO`, and `400`/`404` for traversal, NUL, symlink and
dotfile probes. The alternatives — nginx in front, a manifest full of rewrite regexes,
or a dedicated static process — either reintroduce the configuration the page exists
to delete or add a second supervisor, uid and budget for a read-only file.

## Decision

1. **The responder lives in the scheduler, as the scheduler's own uid.**
   `crates/blastit-scheduler/src/static_asset.rs` resolves, `static_response.rs` builds
   the answer, and `scheduler.rs` dispatches it on the existing `http.chan`. In production that is `UID_BASE+2`
   (`ARCHITECTURE.md` §2.4), with the seccomp role that may not call `socket()`: no new
   process, role, uid, socket, supervisor or filter. A static answer takes one existing
   admission slot (`BLASTIT_MAX_WORKERS`); a `Forward` verdict releases that slot and
   re-enters the ordinary view path, so the route's own lazy spawn and queue apply. It
   keeps no view of its own, reads its chunks on the blocking pool and pumps them under
   the existing response credit. The `request` event
   keeps its shape. A static root must consequently be traversable and readable by the
   scheduler's uid (`0755`/`0644` build output); `check` and `dev` probe it as the
   invoking uid, the production boot as the responder's uid, all before any listener is
   bound and before any role child exists (the only pre-boot fork is the read-only
   probe), and the scheduler's own `open` is the last authority.
2. **Per-route `[route.static]`, four keys only.** `root` (required), `index`
   (`index.html`), `spa` (`false`), `immutable` (none). `static` is a reserved route
   key, like `ingress`. No top-level `[static.<name>]`, no `fallback`, no cache-age
   knob, and one glob metacharacter (`*`, which stops at `/`): the page's block is the
   whole grammar, and `app` being manifest-global makes one `.*` route plus one `app`
   the front-controller config.
3. **Path safety is structural, not string-based.** One dirfd per root opened at boot;
   every candidate is `openat2` with
   `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`, opened `O_PATH`,
   classified by `fstat`, and read-opened `O_RDONLY|O_NOCTTY|O_NONBLOCK` only when it is
   a regular file, with a `(st_dev, st_ino)` identity check between the two opens; a
   per-segment `O_NOFOLLOW` walk is the documented fallback. `canonicalize()` plus a
   prefix test is forbidden: it checks one path and opens another, and follows the
   symlinks an attacker plants. Exactly one percent-decode, no dot-segment resolution
   and no Unicode normalization, so validator and filesystem can never disagree.
   Mounts inside `root` are followed (a bind-mounted asset directory is legitimate);
   the boundary claimed is "beneath `root` in the path namespace".
4. **Precedence is one total order, and a refusal is final.** Exact file → directory
   `index` → SPA `index` (only when `spa = true`) → the manifest's `app` (only when
   `spa` is unset) → `404`, and a lost SPA fallback is that `404`. Refused, hidden and
   lost-fallback paths are terminal for **every** method and body: a `..` probe can
   never be masked by `index.html` (the traversal-to-fallback bug) and never reaches the
   app.
5. **Fail-closed response semantics, and the cache key is the answer kind.** `GET`/`HEAD`
   only on a **servable** selected target (`405` + exactly `Allow: GET, HEAD`); a
   declared body (`Content-Length > 0` or any `Transfer-Encoding`, decided from the head
   alone) is `413`; a refusal keeps its own status whatever the method — `400` for
   traversal/NUL/control/absolute/backslash/invalid UTF-8/over-long, `404` for symlinks,
   special files, oversize files, dotfiles and unreadable files — so a `POST` to a
   refused path is `400`/`404`, never `405`. A lost SPA fallback and a miss with no
   `app` are that same `404`. The
   404-vs-fallthrough distinction does reveal that *something* exists at a refused path
   (a symlink, an unreadable file): that is the page's own "hidden existence is 404"
   policy, it is limited to the status code, and the canned body never varies — content
   is never revealed. `/.well-known/` is the single dotfile exemption. Only an
   **exact-file** answer whose path matches an `immutable` glob gets
   `public, max-age=31536000, immutable`; a directory `index` and an SPA fallback are
   always `no-cache`, so the SPA shell can never be pinned for a year behind a rebuild.
6. **Unsupported paths are refused at boot.** A static root that is missing, not a
   directory, unlistable or unreadable by the responder stops `dev`, `blastit` and
   `check` before any listener is bound, naming the manifest path, the route and the
   root; `app` is optional exactly when every route is static. A static route that can
   never reach the app may not declare an ingress codec, `prefork`, a grant or an inline
   group.

## Consequences

- **Positive.** One config block replaces an nginx `location`/`try_files`/`alias`
  trio; the same code path serves `blastit dev`, presets and CI; the CVE classes that
  depend on path confusion, encoded slashes, NUL truncation, symlink races or device
  opens fail closed by construction; `check` gains a meaningful app-less mode.
- **Trade-offs.** The scheduler now reads files named by a client, so a compromised
  scheduler gains read access to everything beneath a declared `root` (operator-
  declared, dotfiles excluded, dotfiles in `index` refused) — an explicitly stated
  trust assumption, bounded by the scheduler's uid and its socket-less seccomp role.
  Symlinks *inside* a root are refused, so Capistrano-style `current`-symlink layouts
  need the real path in `root`. No range requests, no compression and no redirects in
  `0.2.0`; a file over 64 MiB is not served; `root` is expected on a local filesystem,
  a network mount being unsupported.
