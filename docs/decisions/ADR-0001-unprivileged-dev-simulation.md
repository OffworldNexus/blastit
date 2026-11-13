# ADR-0001: Unprivileged Single-UID `blastit dev` Simulation vs Fail-Closed Production

- **Status:** Accepted
- **Date:** 2026-11-12
- **Node:** MA.5
- **Owner:** architect

## Context

Production `blastit` isolates routes by running `init`, `http`, `scheduler`, `dbproxy`, and each route's `view` process under distinct Linux UIDs (`BLASTIT_UID_BASE + {1, 2, 3, 1000+i}`), backed by either `uid 0` or the 6-capability rootless set (`CAP_SETUID`, `CAP_SETGID`, `CAP_CHOWN`, `CAP_FOWNER`, `CAP_DAC_OVERRIDE`, `CAP_KILL`, plus optional `CAP_SETPCAP`) and per-role `seccomp` filters.

Requiring `sudo`, `setcap` on Python binaries, or `chmod o+rx` on `$HOME` on a developer laptop or CI runner destroys adoption (`docs/VISION.md`). Conversely, replacing `blastit` locally with a single-process WSGI toy would hide routing mismatches, missing proxy grants, `[route.ingress]` failures, seccomp egress blocks, and `BLASTIT_*` / secret-stripping mistakes until production, while silently falling back from production `blastit` to single-UID execution would turn a misconfigured container into an unsandboxed production server.

## Decision

1. **Production `blastit` fails closed (`ExecutionMode::Production`):**
   - `blastit` (with no subcommand) requires `uid 0` or the 6 required capabilities (`blastit_sys::caps::detect()`) and `seccomp`. If privileges are missing, `stage0` emits its JSON `boot` line, prints an error pointing to `blastit dev` for local development and the capability recipe for production, and exits `1`. There is **no** environment variable or fallback path that degrades `blastit` into single-UID mode.
2. **Explicit `blastit dev` single-UID simulation (`ExecutionMode::Dev`):**
   - `blastit dev` runs completely unprivileged as `geteuid()` while executing the **real five-role multi-process topology**:
     - Stage 0 reads `[proxy.*]` secrets, `fork` + `execve`s `dbproxy` with an empty `envp` (`secrets.pipe`, setting `allow_root_listener = (geteuid() == 0)` for root CI containers while production rejects `uid == 0` listeners), and `execve`s Stage 1 with `prepared.app_env()` (stripping real credentials, `BLASTIT_*`, and the variable names listed in `BLASTIT_ENV_STRIP` from `environ` and `argv`).
     - Creates an ephemeral `0700` runtime root (`$XDG_RUNTIME_DIR/blastit-dev-<pid>-<nonce>` when valid, else `/tmp/blastit-dev-<uid>-<pid>-<nonce>` verified against symlink/ownership races) with **per-route subdirectories** `<runtime_root>/views/<route_index>/` (`0700`) so each route's `/proc/self/cwd` holds `.s.PGSQL.<port>` (`0600`, `SO_PASSCRED`) **if and only if** that route has a grant on that proxy.
     - Skips `setresuid`/`setresgid`/`setgroups`, multi-UID path-visibility probes, and per-UID `RLIMIT_NPROC` lowering (which on shared developer UIDs would count desktop/IDE threads and fail `pthread_create` with `EAGAIN`), while keeping `RLIMIT_NOFILE`, `RLIMIT_CORE=0`, `umask 077`, `PR_SET_PDEATHSIG(SIGKILL)`, `PR_SET_NO_NEW_PRIVS(1)`, and **real per-role `seccomp` filters** (which require only `PR_SET_NO_NEW_PRIVS`).
3. **Unmistakable labelling, exit code `1` on usage/exposure errors, and two-key non-loopback guard:**
   - `stage0` `boot` reports `"privilege": "unprivileged-dev"` (preserving `ARCHITECTURE.md` §8.1's rule that no port field appears on log events). Pretty mode prints the 3-line non-sandbox banner on startup; `BLASTIT_LOG_FORMAT` defaults to `pretty` in `dev` (`json` if explicitly set).
   - `blastit dev` binds `127.0.0.1:8080` by default (`--port N` overrides `PORT`; invalid flags, `--preset`, or `--manifest` exit `1` on `stderr` with empty `stdout`). Passing `--host ADDR` with any non-loopback `IpAddr` (`!ip.is_loopback()`) is **refused at startup (`exit 1`)** unless `BLASTIT_DEV_EXPOSE=1` is also set; when exposed, `stage0` emits a loud `WARN` naming `<host>:<port>`, `uid`, and the absence of authentication or multi-UID sandboxing.

## Consequences

- **Positive:** Zero-`sudo` 3-step local onboarding (`uv add blastit` -> `blastit.toml` -> `uv run blastit dev`); honest programs hit real routing, ingress codecs, secret stripping, per-route socket presence, SQL allowlists, and seccomp `egress` rules locally.
- **Trade-offs & Honest Boundaries:** Same-UID processes can read `.env` and `/proc/<pid>` on Linux (`docs/security.md#dev-mode-is-not-a-sandbox`); docs and the startup banner explicitly warn that `blastit dev` is not a sandbox and must only connect to a throwaway local database.
