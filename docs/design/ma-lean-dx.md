---
status: Accepted
owner: architect
node: MA.5
reviewers: [implementability, testability, consistency]
---
# Blastit 0.1.1 Technical Design: Zero-`sudo` `blastit dev`, Auto-Manifest, Wire `--learn`, and Framework-Agnostic `blastit check`

This document translates the accepted `0.1.1` user documentation (`README.md`, `docs/**`) into concrete Rust crate boundaries, type/trait contracts, wire protocols, security invariants, and an ordered `MA.2` implementation plan. Every flag, TOML key, exit code, stdout/stderr guarantee, and JSON field specified in the user documentation is normative.

Companion ADRs:
- [ADR-0001](../decisions/ADR-0001-unprivileged-dev-simulation.md): Unprivileged Single-UID `blastit dev` Simulation vs Fail-Closed Production
- [ADR-0002](../decisions/ADR-0002-manifest-discovery-and-precedence.md): Manifest Auto-Discovery, App Wiring Keys, and Setting Precedence
- [ADR-0003](../decisions/ADR-0003-wire-learn-and-literal-redaction.md): Wire-Level `--learn`, SQL Literal Redaction/Refusal, and Atomic Manifest Writes
- [ADR-0004](../decisions/ADR-0004-framework-agnostic-check-and-json-contract.md): Framework-Agnostic `blastit check` Architecture and `--json` Machine Contract
- [ADR-0005](../decisions/ADR-0005-deployment-presets-and-seccomp-gate.md): Deployment Presets as Overridable Defaults with Mandatory Seccomp Gate
- [ADR-0006](../decisions/ADR-0006-route-ingress-body-gate.md): Fail-Closed `[route.ingress]` Body Validation and Canonical Re-Encoding

---

## 1. Developer Experience Target (Local, CI/CD, Deploy)

### 1.1 Command Surface, Stdout/Stderr, and Exit Codes

| Command | Flags | Stdout Contract | Stderr Contract | Exit Codes | Source Docs |
|---|---|---|---|---|---|
| `blastit` | `--preset NAME` / `--preset=NAME`, `-h` / `--help` (plus internal `--stage=1`, `--role=dbproxy`) | Server event stream only (`BLASTIT_LOG_FORMAT`, default `json`), 1 event/line ≤ 2048 B (`--help` prints usage + presets to stdout) | Fatal stage-0 pre-log or child `_exit` diagnostics (`0` bytes on stdout when CLI parsing fails) | `0` clean `SIGTERM`/`--help`; `1` CLI/boot/privilege/seccomp/runtime failure | `docs/reference/cli.md`, `docs/deploy/index.md` |
| `blastit dev` | `--port N` (`8080`), `--host ADDR` (`127.0.0.1`), `--learn`, `-h` / `--help` | Pretty event stream by default (`BLASTIT_LOG_FORMAT` overrides) + 3-line startup banner when format is `pretty`; pure JSONL when `BLASTIT_LOG_FORMAT=json`; on `--learn` clean exit (`pretty`), prints `learn: …` summary | CLI/startup/exposure errors (`0` bytes on stdout); `--learn` refused-statement diagnostics (plus `learn: …` when `BLASTIT_LOG_FORMAT=json` so stdout stays pure JSONL) | `0` clean `SIGINT`/`SIGTERM` (with zero refused `--learn` statements); `1` CLI/config/manifest/exposure error or refused `--learn` statement | `README.md`, `docs/getting-started.md`, `docs/local.md`, `docs/reference/cli.md` |
| `blastit check` (`blastit test` alias) | `--json`, `--learn`, `--dry-run` (with `--learn`), `-- ARGS…`, `-h` / `--help` | Without `--json`: pytest output + human `blastit check: …` or `learn: …` summary. **With `--json`:** **at most one JSON document** on `stdout` (`0` bytes on exit `2` or `3`, enforced via OS fd redirection of subprocess stdout -> stderr) | With `--json`: all pytest output, Blastit logs, progress, refused-statement reasons, and `learn:` lines | `0` clean (or `--learn` succeeded with ≥ 1 request); `1` completeness findings (or `--learn` refused a statement / observed `0` requests, `"wrote": false`); `2` Blastit CLI/config/manifest/missing-proxy-input/missing-`pytest`/corrupt-obs error; `3` pytest exited non-zero **including pytest exit `5` ("no tests collected")** | `docs/ci.md`, `docs/reference/cli.md`, `docs/troubleshooting.md` |
| `blastit-manifest fold` (`python -m blastit.fold`) | `LOG…` (`-` = stdin), `--manifest PATH` (required), `--write`, `--prefer-tables`, `--allow-dangerous` | Pasteable TOML diff (empty when `--write`) | Refused statements (inline literals, multi-statements, non-DML, unsafe functions) + diagnostics | `0` readable log + manifest (whether diff is empty, partial after refusals, or full); `1` unreadable log/manifest | `docs/reference/cli.md`, `docs/reference/manifest.md` |

### 1.2 Startup Banner (`blastit dev` in `pretty` format)

```text
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

- Line 3 pluralizes `1 route` vs `<N> routes`.
- When `!host.is_loopback()` (permitted **only** with `BLASTIT_DEV_EXPOSE=1`), line 1 reads `  blastit dev · single-uid simulation · <host>:<port> · uid <euid> (EXPOSED, no auth)` and `stage0` emits a `WARN` event naming `<host>:<port>` and `<euid>`.

### 1.3 Human & `--json` Output Contracts (`blastit check`)

- **Clean human line (`docs/local.md`):** `blastit check: <R> requests, <S> statements, <E>/<T> routes[, <P> proxies], clean` (includes `, <P> proxies` iff `proxies.len() > 1`).
- **Findings human block (`docs/ci.md`):**
  - Header: `blastit check: <R> requests, <S> statements, <E>/<T> routes[, <P> proxies]`
  - Per-finding lines (exact 9-row mapping):
    1. `missing_grant`: `  route <R>: <N> statement[s] not granted` followed by indented `    <redacted_sql>` per statement on `<R>`
    2. `dead_grant`: `  route <R>: <N> grant[s] never used` (or `  group <G>: <N> grant[s] never used` for group grants)
    3. `unexercised_route`: `  route <R>: never exercised`
    4. `missing_ingress`: `  route <R>: takes a body and declares no ingress validation`
    5. `ingress_mismatch`: `  route <R>: declared body = "<D>" does not match observed <M>`
    6. `raw_ingress`: `  route <R>: declares body = "raw" without allow_raw = true`
    7. `unrouted_request`: `  request <M> <P>: no route`
    8. `no_observations`: `  suite exercised 0 routes or 0 proxy statements`
    9. `proxy_unused`: `  proxy <P>: declared, 0 statements observed`
  - Footer: `blastit check: <F> problem[s] · \`--json\` for the machine-readable list` (`F` = total `findings.len()`).
- **`--learn` human line (`docs/local.md`):** `learn: <R> routes, <Q> queries added → <abs_path>` (`proposed (dry-run)` when `--dry-run`; `<abs_path>` is lexical `std::path::absolute(&config.manifest)` without resolving symlinks).
- **`--json` contracts:** Audit JSON (`schema_version: 1`) and `--learn --json` (`{"proposed": {"routes": R, "queries": Q}, "wrote": true|false}`, intentionally omitting `schema_version` to match `docs/ci.md:144` verbatim) are specified in §4.2 E.

---

## 2. Goals & Non-Goals

### Goals
1. **Zero-`sudo` local simulation (`blastit dev`)** running the real 5-role process tree, stage-0 `execve` secret stripping, per-route isolated Unix socket directories, `[route.ingress]` codecs, SQL policy enforcement, and unprivileged `seccomp`.
2. **Fail-closed production (`blastit`)** requiring `uid 0` or the 6-capability set plus `seccomp` (with preset hard-abort on `BLASTIT_SECCOMP=0`), never degrading to single-UID execution.
3. **Minimal configuration (`./blastit.toml`)** with CWD-only discovery, non-secret `app` and `preload` keys, `[route.ingress]` body codecs (`json`, `form`, `none`, `raw`), reserved-key-in-table validation, and deterministic `BLASTIT_*` override precedence.
4. **Single-pass wire `--learn` & framework-agnostic `blastit check`** across Django, Flask, SQLAlchemy, and raw WSGI without root, orchestrated by Rust over `blastit-dbproxy` with literal redaction (`…`), literal-grant refusal, unified surgical TOML editing + atomic writes, and a guaranteed-bootable `--json` `fix` contract.

### Non-Goals & Scheduled Sibling/Follow-Up Scope
- Non-Python backends (PHP-FPM, Vite/Node, Tomcat) -> **Milestone B (`0.2.x`)**; WASM CDR (`multipart`, XML, media, PDFs) -> **Milestone C (`0.3.x`)**; MySQL/Mongo/Redis/HTTP proxies -> **Milestone D (`0.4.x`)**.
- Per-route `max_body` overrides (`BLASTIT_MAX_BODY_BYTES` remains one global 100 MiB ceiling in `0.1.1`).
- **Scheduled for `MA.3` (which already owns `README.md`, `ARCHITECTURE.md`, `docs/**`, `python/tests/**`, `e2e/**` right after `MA.2`):**
  1. Replace illustrative `SELECT … FROM django_session` in `docs/reference/manifest.md:48` with a concrete column list so no doc example contains `…`.
  2. Clarify `docs/local.md:114` ("for a parameterised statement, exact grant text; a redacted `…` must be parameterised first") and note the two `KNOWN_STATIC_SETUP_SQL` timezone strings in `docs/reference/manifest.md:188`.
  3. Clarify `docs/ci.md:99` (stdout is empty on both exit `2` and exit `3`), `docs/ci.md:83-86` (group `dead_grant` `fix.to` + `observe` on ungrantable redacted/tx denials), and `docs/deploy/index.md:63` (`config` `boot` line carries `preset`).
  4. Sync `ARCHITECTURE.md` (§6 `./blastit.toml`, `--learn`, unprivileged `blastit_live_server`; §8.1 `preload` `build`; §8.2 `dev`/`check` & precedence) and document `blastit_live_server`'s non-root `blastit dev` execution.
- **Post-`MA` CI/Tooling Follow-Ups (outside `MA.{2,3}` paths):** add `blastit check --json` workflow step in `.github/workflows/ci.yml` and mechanical critical-skip guard in `tools/guard.py`.

---

## 3. Requirements (`R1..R42`)

### CLI & Subcommands
- **R1 (`blastit` production CLI):** `blastit` (no subcommand) accepts `--preset NAME` / `--preset=NAME` and `-h` / `--help` in stage 0, rejects `--preset` on `--stage=1` and `--role=*`, rejects unknown flags/arguments with exit `1` (message on `stderr`, `0` bytes on `stdout`), and prints no `--version` (`preload` `boot` carries `build`). (`docs/reference/cli.md`)
- **R2 (`blastit dev` CLI):** `blastit dev` accepts `--port N` / `--port=N` (`1..=65535`, default `8080`, overriding `PORT`), `--host ADDR` / `--host=ADDR` (default `127.0.0.1`), `--learn`, and `-h` / `--help`; rejects `--manifest`, `--preset`, and invalid port/IP arguments with exit `1` on `stderr` and `0` bytes on `stdout`, and ignores `BLASTIT_PRESET`. (`docs/local.md`, `docs/reference/cli.md`)
- **R3 (`blastit check` / `blastit test` CLI):** `blastit check` and its exact alias `blastit test` accept `--json`, `--learn`, `--dry-run` (valid only with `--learn`; error exit `2` otherwise), `-h` / `--help`, and `-- ARGS…` forwarded to `pytest -q`; reject `--manifest`, `--preset`, unknown flags, and forwarded `--blastit-manifest` / `--blastit-live-server` flags with exit `2` on `stderr` and `0` bytes on `stdout`, and ignore `BLASTIT_PRESET`. (`docs/ci.md`, `docs/reference/cli.md`)
- **R4 (`blastit check` exit codes):** `blastit check` exits `0` when `pytest` passed (`rc == 0`) and zero `error`-severity findings exist (or `--learn` observed ≥ 1 HTTP request with zero refused statements), `1` when `pytest` passed and ≥ 1 `error`-severity finding exists (or `--learn` refused ≥ 1 statement or observed `0` HTTP requests, leaving the manifest untouched with `"wrote": false`), `2` on CLI/config/manifest/missing-proxy-input/missing-`pytest`/corrupt-obs failure, and `3` when `pytest` exits non-zero **including `pytest` exit `5` ("no tests collected")**. (`docs/ci.md#exit-codes`, `docs/troubleshooting.md`)
- **R5 (`--json` stdout purity):** Under `blastit check --json` (with or without `--learn` / `--dry-run`), `stdout` receives **at most one JSON document** (`0` bytes on exit `2` or `3`), enforced by redirecting the `pytest` child's OS file descriptor `1` onto `stderr`. (`docs/ci.md#for-agents-json`, `docs/reference/log-events.md`)

### Manifest Discovery, Schema & Precedence
- **R6 (CWD-only `./blastit.toml` discovery):** When `BLASTIT_MANIFEST` is unset, `blastit`, `blastit dev`, and `blastit check` read `./blastit.toml` from the starting working directory with **zero upward directory search**. `blastit-manifest fold` continues to require `--manifest PATH`. (`docs/reference/cli.md`, `docs/reference/manifest.md`)
- **R7 (Top-level `app` and `preload`):** `blastit.toml` supports optional top-level `app` (`"module:attribute"`) and `preload` (either a single string `"mod"` or an array of strings `["mod1", "mod2"]`; `BLASTIT_PRELOAD` remains comma-separated). At least one of `manifest.app` or `BLASTIT_APP` must be present; invalid `module:attribute` strings fail with a message containing ``must be `module:attribute` ``. (`docs/reference/manifest.md`, `docs/troubleshooting.md`)
- **R8 (Reserved top-level key guard):** In `blastit-manifest/src/parse.rs` (`toml::Table` walk), `TOP_LEVEL_KEYS` adds `"app"` and `"preload"`. Whenever `"version"`, `"app"`, or `"preload"` appears as a key inside any `[table]` or `[[route]]`, or `"groups"` appears inside any table other than `[[route]]` (`[proxy.*]`, `[group.*]`, `[route.<proxy>]`, `[route.ingress]`), parsing fails with `<key> after a [table] is not top-level; move it above the first table`. (`docs/reference/manifest.md#top-level-keys`)
- **R9 (Two-layer precedence & empty-string override):**
  1. *Preset selector (`blastit` production only):* `--preset NAME` overrides `BLASTIT_PRESET` (`Warning::PresetFlagWins`; `BLASTIT_PRESET` retains whitespace-unset semantics).
  2. *Setting values:* `built-in default < active preset's values < manifest (app, preload) < set environment variable (BLASTIT_*, PORT) < value CLI flags (dev --port, dev --host)`. Any set `BLASTIT_*` or `PORT` variable (`std::env::var_os(k).is_some()`) overrides presets and the manifest **even when `""`** (`BLASTIT_PRELOAD=""` yields `[]`; `BLASTIT_APP=""` and `BLASTIT_MANIFEST=""` fail validation). (`docs/reference/settings.md`, `ADR-0002`, `ADR-0005`)
- **R10 (`BLASTIT_LOG_FORMAT` per-command default):** `BLASTIT_LOG_FORMAT` defaults to `json` in `blastit` and to `pretty` in `blastit dev` and `blastit check`; an explicit `BLASTIT_LOG_FORMAT` variable wins in all three. Early stage-0 lines wait for config parsing and fall back to JSON if config parsing fails. (`docs/reference/settings.md#logs`, `docs/reference/log-events.md`)

### `[route.ingress]` & HTTP Body Gate
- **R11 (`[route.ingress]` manifest schema & boot warnings):** `"ingress"` joins `ROUTE_KEYS` as an optional subtable with required `body = "json" | "form" | "none" | "raw"`; when `body == "raw"`, it also accepts optional `allow_raw: bool` (default `false`; rejected as `ManifestError::UnknownKey` on `json`/`form`/`none`). `Manifest::validate(&self, base: u32)` emits `ManifestWarning::RawIngress` (`body = "raw"`) and `ManifestWarning::EgressRoute` (`egress = true`), rendered as `manifest.warning` boot events. (`docs/reference/manifest.md#ingress`, `#validation-at-boot`)
- **R12 (Runtime `413` on undeclared or `none` body & empty-body rules):** In `blastit` and `blastit dev` (when `--learn` is off), any request with `> 0` body bytes matching a route with no `[route.ingress]` or with `body = "none"` is rejected by `blastit-scheduler` with `413` before Python runs (`Content-Length > BLASTIT_MAX_BODY_BYTES` and chunked streams exceeding `BLASTIT_MAX_BODY_BYTES` are rejected at `blastit-http` with `413`). Zero-byte bodies follow §4.2 A. (`docs/reference/manifest.md#ingress`, `ADR-0006`)
- **R13 (Early HTTP rejection without upload deadlock):** When `blastit-scheduler` responds early (`400`, `404`, `413`, `503`, `504`) while `blastit-http` is reading a client body or waiting for `INITIAL_WINDOW` credit, `blastit-http` stops reading the upload immediately via `Stream.pending`, appends `Connection: close`, flushes the response, and closes the client socket without waiting for `body_timeout`. (`crates/blastit-http`, `ADR-0006`)
- **R14 (`body = "json"` canonical decode/re-encode & header scrub):** Rejects non-identity `Content-Encoding` (`400`); requires `Content-Type` essence `application/json` or `application/*+json` (case-insensitive `charset=utf-8` or omitted); validates strict UTF-8 RFC 8259 JSON (max depth `64`, rejecting UTF-8 BOM, trailing bytes, **duplicate object keys**, and **numeric lexemes outside exact `i64`/`u64`/finite-`f64`**); re-serializes compact UTF-8 JSON with sorted `BTreeMap` keys and scrubs framing headers via `apply_transcoded_headers`; any violation returns `400`. (`docs/reference/manifest.md#ingress`, `ADR-0006`)
- **R15 (`body = "form"` canonical decode/re-encode & header scrub):** Rejects non-identity `Content-Encoding` (`400`); requires `Content-Type` essence `application/x-www-form-urlencoded` (case-insensitive `charset=utf-8` or omitted); splits on `&` (`;` is literal), validates `%XX` hex pairs + strict UTF-8 via `percent-encoding` with **zero NUL (`\0`) bytes**, re-serializes via `url::form_urlencoded::Serializer`, and scrubs framing headers via `apply_transcoded_headers`; any violation returns `400`. (`docs/reference/manifest.md#ingress`, `ADR-0006`)
- **R16 (`body = "raw"` zero-buffer streaming):** `plan_body` returns `BodyPlan::StreamPassthrough` on `ReqHead`, streaming body frames directly through `1 MiB` flow-control windows with `0` bytes buffered in `blastit-scheduler`. (`docs/reference/manifest.md#ingress`, `ADR-0006`)

### Unprivileged `blastit dev` vs Production Fail-Closed & Presets
- **R17 (Production fail-closed):** `blastit` refuses to start (`exit 1` on `stderr`, naming `blastit dev` and the capability recipe) whenever `blastit_sys::caps::detect()` fails or `seccomp` installation fails; zero code paths degrade `blastit` to single-UID execution. (`docs/deploy/index.md`, `docs/security.md`, `ADR-0001`)
- **R18 (`BLASTIT_SECCOMP=0` gate & presets):** `BLASTIT_SECCOMP` defaults to `1`. In `blastit`, when `BLASTIT_SECCOMP=0`, startup **aborts (`exit 1`)** if any preset (`digitalocean-paas` or `compose-traefik`) is active, and otherwise emits `Warning::SeccompDisabled` (using the same stage-0 warning stream as `Warning::PresetExpectsRoot` / `PresetExpectsRootless`). Presets supply defaults (`BLASTIT_CLIENT_IP_HEADER`, `BLASTIT_TRUSTED_PROXIES=*`, `BLASTIT_FAIR_QUEUING=on`, `BLASTIT_LOG_FORMAT=json`), log `preset`, `preset_settings`, and `preset_overridden` in `config` `boot`, and abort with the preset list on unknown names. (`docs/reference/settings.md`, `docs/deploy/*`, `ADR-0005`)
- **R19 (`blastit dev` real 5-role single-UID topology):** `blastit dev` runs `init` (stage 0 -> `execve` stage 1), `dbproxy` (`fork` + `execve` with empty `envp`), `http`, `scheduler`, and lazy/prefork `view` processes under `geteuid()` without `sudo`, file capabilities, or `chmod` on `$HOME`. (`docs/local.md`, `docs/how-it-works.md`, `ADR-0001`)
- **R20 (`blastit dev` stage-0 `execve` barrier & per-route runtime dirs):** `blastit dev` strips all `[proxy.*]` secret inputs, all `BLASTIT_*` variables, and the variable names listed in `BLASTIT_ENV_STRIP` across the stage-0 `execve` barrier, and creates an ephemeral `0700` runtime root (`$XDG_RUNTIME_DIR/blastit-dev-…` or `/tmp/blastit-dev-…`, symlink/ownership-verified, cleaned up on exit) with per-route subdirectories `<runtime_root>/views/<route_index>/` (`0700`) so each route gets `.s.PGSQL.<port>` (`0600`, `SO_PASSCRED`) **only** for proxies it is granted (or all declared proxies under `--learn`). (`docs/local.md`, `docs/how-it-works.md`, `ADR-0001`)
- **R21 (`blastit dev` child hardening & `RLIMIT_NPROC` safety):** `blastit dev` children set `umask 077`, `RLIMIT_NOFILE`, `RLIMIT_CORE=0`, `PR_SET_PDEATHSIG(SIGKILL)`, `PR_SET_NO_NEW_PRIVS(1)`, and **real per-role `seccomp` filters** (enforcing `egress = false` vs `true`), while omitting `setresuid`/`setgroups` and omitting per-UID `RLIMIT_NPROC` lowering. (`ADR-0001`)
- **R22 (`blastit dev` banner, `privilege` field & `BLASTIT_DEV_EXPOSE`):** `blastit dev` emits `"privilege": "unprivileged-dev"` in `stage0` `boot` (never emitting a port field in any log event, preserving `ARCHITECTURE.md` §8.1), prints the 3-line banner in `pretty` mode, binds `127.0.0.1` by default, and refuses any non-loopback `--host` with `exit 1` (on `stderr`, `0` bytes on `stdout`) unless `BLASTIT_DEV_EXPOSE=1` is set (logging a loud `WARN` naming `<host>:<port>` and `uid` when exposed). (`docs/local.md`, `docs/reference/settings.md`, `ADR-0001`)
- **R23 (`preload` `boot` `build` field):** `Lifecycle` carries optional `build: Option<String>` set **only** when constructing the `preload` `boot` event (`env!("CARGO_PKG_VERSION")`), omitted (`None`) on `stage0`/`config`/`ready`/`shutdown`, and round-tripping through `blastit-events` JSON/pretty codecs. (`docs/reference/cli.md`, `docs/reference/log-events.md`)

### SQL Literal Redaction & Wire `--learn`
- **R24 (SQL literal redaction in logs/JSON & manifest rejection):** Every `sql.denied` event, internal `obs_fd` record, and `blastit check` finding replaces all 8 PostgreSQL literal forms (`'…'`, `E'…'`, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`, `$tag$…$tag$` — replacing the **entire token including prefix** with `…`, e.g. `SELECT E'\n'` -> `SELECT …`, `SELECT B'1'` -> `SELECT …`, `SELECT U&'\0041'` -> `SELECT …`, `SELECT $t$x$t$` -> `SELECT …`) while preserving `"quoted_identifiers"`, `U&"quoted_identifiers"`, `$?`, and exact matches in `KNOWN_STATIC_SETUP_SQL`. `Manifest::validate` rejects any `queries` entry containing `…` (`ManifestError::RedactedQueryPlaceholder`) or an inline quoted literal outside `KNOWN_STATIC_SETUP_SQL` (`ManifestError::LiteralQuery`). (`docs/local.md#a-denial`, `docs/reference/manifest.md#grants`, `ADR-0003`)
- **R25 (Literal & server-escape SQL refusal in `--learn` and `fold`):** `blastit check --learn`, `blastit dev --learn`, and `blastit-manifest fold` unconditionally refuse unparseable SQL (`RefuseReason::Unparseable`), multi-statement strings, and any statement containing an inline quoted literal or `…` outside `KNOWN_STATIC_SETUP_SQL`. `--learn` additionally refuses `COPY … PROGRAM` and `DANGEROUS_LEARN_FUNCTIONS`; `fold` retains its `classify` + `SAFE_FUNCTIONS` (`--allow-dangerous`) policy and documented `0`/`1` exit codes. (`docs/local.md`, `docs/ci.md`, `docs/reference/cli.md`, `ADR-0003`)
- **R26 (Non-short-circuiting execution in `check` & `dev --learn`):** `blastit check` (audit and `--learn`, via `BlastitWsgiGate` + `dbproxy` `DynamicCheck`) and `blastit dev --learn` (via `scheduler` + `dbproxy` `Learn`) never abort requests on unrouted paths, missing `[route.ingress]`, or ungranted SQL (`dbproxy` always returns `QueryDecision::Allow` and writes `WireSqlObs` to internal `obs_fd` without emitting `sql.denied` on stdout). In `dev --learn`, `BLASTIT_MAX_BODY_BYTES` (`413`) and explicitly declared `[route.ingress]` codecs remain enforced. (`docs/local.md`, `docs/ci.md`, `ADR-0003`, `ADR-0004`)
- **R27 (Exact `queries`-only learning & unrouted query assembly):** `--learn` writes **only** inferred `[[route]]` blocks and exact normalized `queries` (mapping `ObsTarget::Unrouted { method, path }` through `infer_routes` to its proposed route's `[route.<proxy>].queries`, deduplicated against effective grants; never writing `tables` grants or silently mutating `transaction_control`; ungranted transaction-control commands emit a stderr hint). (`docs/ci.md`, `docs/frameworks/flask.md`, `ADR-0003`)
- **R28 (Route inference in `--learn`):** Unrouted requests observed during `--learn` propose new `[[route]]` blocks per §4.2 E.4 (first static segment lowercased, non-`[a-z0-9_-]` runs collapsed to `-`, **both leading and trailing** non-`[a-z0-9]` stripped, fallback `"route"`, `"/"` -> `"root"`, disambiguated with `-2`, `-3`, …; pure ASCII digits -> `[0-9]+`; UUIDs -> `[^/]+`), inserted by parsed route index immediately above the first method-unrestricted `.*` catch-all route (matching both `path = ".*"` and `path = ['.*']`) or at EOF. (`docs/local.md`, `ADR-0003`)
- **R29 (Unified surgical TOML edit, atomic write & `--learn --json`):** `crates/blastit-manifest/src/edit.rs` preserves comments/formatting across `dev --learn` and `check --learn`, verifies untouched sections, and writes via `.{filename}.{pid}.tmp` + `fsync` + `rename` + dir `fsync` (`fold --write` uses the same temp-file + `fsync` + `os.replace` pattern). `check --learn` writes **only** when `pytest` exited `0`, `obs.http.len() > 0`, zero statements were refused, `!dry_run`, and `(proposed.routes > 0 || proposed.queries > 0)`; `check --learn --json` prints `{"proposed": {"routes": R, "queries": Q}, "wrote": true|false}` on `stdout`. (`docs/ci.md`, `docs/reference/cli.md`, `ADR-0003`)

### Framework-Agnostic `blastit check` & 4-Way Completeness
- **R30 (Hermetic WSGI + `dbproxy` audit):** `blastit check` (`crates/blastit/src/check.rs`) spawns `pytest -q -p no:blastit -p blastit._check_pytest` unprivileged against Django, Flask, SQLAlchemy, and raw WSGI with `blastit-dbproxy` on `<runtime_dir>/pg`, `swapped_env` keeping real secrets out of non-Django `pytest` processes (and unlinking a one-shot `0600` setup file right after `django_db_setup` for Django), and a `ContextVar`-guarded `BlastitWsgiGate` pushing `ObsTarget::Routed` or `ObsTarget::Unrouted` over `ctl.sock` once per request. (`docs/ci.md`, `ADR-0004`)
- **R31 (Migration/fixture/teardown SQL exemption & introspection replay):** SQL executed while `active_route_stack` is empty executes un-audited (`QueryDecision::Allow`, zero `obs_fd` records, zero statement counts), while connection-init `SET`, `SHOW`, `KNOWN_STATIC_SETUP_SQL`, and read-only `pg_catalog` introspection `SELECT`s (`pg_catalog.version()`, `current_schema()`, `pg_type`/`to_regtype`) are recorded per session (max 8) and replayed once when that connection first executes under a routed target. (`docs/ci.md#your-suite`, `docs/frameworks/flask.md`, `ADR-0004`)
- **R32 (`unrouted_request` finding):** Every observed HTTP request in `CheckObsV1.http` matching no route in `RoutingTable` emits `{"kind": "unrouted_request", "severity": "error", "method": M, "path": P, "fix": {"action": "add_route"}}` (deduplicated by `(method, path)`). SQL under `ObsTarget::Unrouted` increments `statements`/`denied` counts without emitting bogus route-less `missing_grant` entries. (`docs/ci.md`)
- **R33 (`missing_grant` finding & bootable `fix` guarantee):** Every denied SQL statement on `ObsTarget::Routed { route }` emits `missing_grant` (deduplicated by `(route, proxy, sql)`). When `sql` is a grantable parameterized query, `"fix"` is `{"action": "add_query", "to": "[route.<proxy>].queries"}` — except for connection setup (`KNOWN_STATIC_SETUP_SQL`, or a single `SET`/`SHOW` passing `is_introspection_setup_sql`), which emits `{"action": "add_group", "to": "[group.connection.<proxy>].queries"}` because it runs on whichever route opens the connection first, so a per-route grant would leave every other route denying it (`MA.8`); when `sql` contains `…` (`has_quoted_literal`) or is transaction control (`BEGIN`/`COMMIT`/`ROLLBACK`/`SAVEPOINT`/`RELEASE`, with savepoint names normalized to `"SAVEPOINT"`), `"fix"` is `{"action": "observe"}` so **every emitted `add_query`/`add_group` is guaranteed to pass `Manifest::parse_and_validate`**. (`docs/ci.md`, `ADR-0003`, `ADR-0004`; the `add_group` oracle and its per-route twin are pinned in `crates/blastit/tests/critical_check_engine.rs`)
- **R34 (`dead_grant` finding):** On exercised routes, every inline `[route.<proxy>].queries` entry never matched during the run (and not covered by `[route.<proxy>.tables]` or attached groups via shared `blastit_manifest::is_covered_by_tables_or_groups`) emits `dead_grant` (`"to": "[route.<proxy>].queries", "unsafe": true`). Attached group queries unused across all attaching routes (when ≥ 1 was exercised) emit once with `"group": G` and `"to": "[group.<G>.<proxy>].queries"`. Unexercised routes emit `unexercised_route` once with zero `dead_grant` entries. (`docs/ci.md`)
- **R35 (`unexercised_route` finding):** Whenever `obs.http.len() > 0`, every declared `[[route]]` with zero matched requests emits `{"kind": "unexercised_route", "severity": "error", "route": R, "fix": {"action": "add_test"}}`. When `obs.http.is_empty()`, `unexercised_route` is suppressed in favour of `no_observations`. (`docs/ci.md`)
- **R36 (`missing_ingress`, `ingress_mismatch`, and `raw_ingress` findings):** Validated in Rust via `blastit-ingress` over `64 KiB`-capped `BodySample`s (truncated bodies > `64 KiB` fall back to `codecs: ["raw"], allow_raw: true`): emits `missing_ingress`, `ingress_mismatch`, and static `raw_ingress` per §4.2 E.3. (`docs/ci.md`, `docs/reference/manifest.md#ingress`)
- **R37 (`proxy_unused` and `no_observations` findings):** Every declared `[proxy.<P>]` with `proxies[P].statements == 0` always emits `proxy_unused`. When `routes.exercised == 0` or (`!manifest.proxies.is_empty()` and `statements == 0`), `blastit check` emits `no_observations`. (`docs/ci.md`)
- **R38 (`blastit check --json` count & schema invariants):** `schema_version == 1`; `statements == sum(proxies[p].statements)`; `ok == findings.iter().all(|f| f.severity != Severity::Error)`; `fix.action` is restricted to `{add_query, add_group, remove_query, add_route, remove_route, add_test, declare_ingress, fix_ingress, observe}` with `"unsafe": true` on `remove_query` and reserved `remove_route`. (`docs/ci.md`)

### Code Hygiene & Build Gates
- **R39 (`blastit_live_server` unprivileged support):** `python/blastit/pytest_plugin.py`'s `blastit_live_server` spawns `blastit dev` when `geteuid() != 0` (never skipping for lack of `sudo`). (`python/blastit/pytest_plugin.py`)
- **R40 (Zero third-party Python runtime dependencies):** `pyproject.toml` `dependencies == []` (verified by critical test loading `pyproject.toml` via `tomllib` and checking `python/blastit/*.py` imports). (`skill python-standards`)
- **R41 (Ruff lint & format gate on `python/`):** All files in `python/` and `tools/` pass `uvx --offline ruff check python tools` and `uvx --offline ruff format --check python tools`, wired into `Makefile` `py-lint` (a prerequisite of `make check`). (`plan/nodes/MA/MA.2/NODE.md`)
- **R42 (Offline, zero-skip `make test-critical` & mutation gate):** `Makefile` `test-critical` rejects skip attributes/calls (`#\[\s*ignore`, `\bneeds_root\s*\(`, `pytest\.mark\.skip`, `pytest\.skip\s*\(`, `importorskip\s*\(`, `unittest\.skip`), builds & stages `_blastit<EXT_SUFFIX>` + `_bin/blastit-dbproxy` with `RUST_ENV`, runs `python -m unittest discover` offline (`CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check`), and targets ≥ 75% `cargo-mutants` kill rate on `blastit-ingress`, `blastit-sqlpolicy`, and `blastit-manifest`. (`Makefile`, `skill testing`)

---

## 4. Architecture & Data Flow

### 4.1 Crate & Module Boundaries & `Cargo.toml` Matrix

- `crates/blastit-ingress/Cargo.toml` (NEW, `#![forbid(unsafe_code)]`, inherits workspace metadata + `[lints] workspace = true`): `percent-encoding = "2.3.2"`, `serde = { version = "1.0.229", features = ["derive"] }`, `serde_json = "1.0.154"`, `thiserror = "2.0.21"`, `url = "2.5.8"` (no `arbitrary_precision`).
- `crates/blastit-manifest/Cargo.toml`: already depends on `blastit-sqlpolicy`; adds `blastit-ingress = { path = "../blastit-ingress" }`, `serde = { version = "1.0.229", features = ["derive"] }` (acyclic: `sqlpolicy` + `ingress` -> `manifest`).
- `crates/blastit-scheduler/Cargo.toml`: adds `blastit-ingress = { path = "../blastit-ingress" }`, `serde_json = "1.0.154"`.
- `crates/blastit/Cargo.toml`: adds `blastit-ingress = { path = "../blastit-ingress" }`, `blastit-sqlpolicy = { path = "../blastit-sqlpolicy" }`, `serde = { version = "1.0.229", features = ["derive"] }`.
- `crates/blastit-py/Cargo.toml`: adds `blastit-ingress = { path = "../blastit-ingress" }`.
- `crates/blastit-frame`, `crates/blastit-sys`, and `crates/blastit-view` require **zero** Cargo or source changes (`blastit-env` gains one pure helper `build_with_socket_dir` in Leaf 1).

### 4.2 Exact Rust Signatures & Wire Contracts

#### A. `crates/blastit-ingress` (`#![forbid(unsafe_code)]`)

```rust
pub const MAX_JSON_DEPTH: usize = 64;
pub const MAX_MIME_REPORT_BYTES: usize = 64;
pub const MAX_CHECK_SAMPLE_BYTES: usize = 64 * 1024; // 64 KiB cap for check body probing

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyCodec { Json, Form, None, Raw }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressPolicy { pub body: BodyCodec, pub allow_raw: bool }

/// Head-level routing plan computed BEFORE reading body frames (preserves R16 zero-buffer Raw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPlan {
    /// `BodyCodec::Raw`: dispatch on `ReqHead` and stream via 1 MiB credit windows (0 B scheduler buffer).
    StreamPassthrough,
    /// `None` / `BodyCodec::None` with no positive `Content-Length`: allow 0 B (`ReqEnd`), `413` on any `ReqBody` byte.
    AllowEmptyOnly,
    /// `BodyCodec::Json` or `BodyCodec::Form` (or bodyless safe verb): buffer up to `max_body_bytes` then `transcode_buffered`.
    Buffer { codec: BodyCodec },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySample {
    pub mime: String,       // lowercase essence ("application/json", "multipart/form-data", "none"), <= 64 ASCII B
    pub truncated: bool,    // true when body_len > MAX_CHECK_SAMPLE_BYTES (forces valid_json=false, valid_form=false)
    pub valid_json: bool,
    pub valid_form: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodedBody {
    pub bytes: Vec<u8>,
    pub content_type: Option<&'static str>,
}

/// Parses `Content-Type` into `(lowercase_essence_capped_64b, optional_lowercase_charset)`.
/// Parameters other than `charset` (such as `boundary=...`) are stripped from `essence`.
pub fn parse_mime(content_type: Option<&str>) -> (String, Option<String>);

pub fn plan_body(
    method: &str,
    policy: Option<IngressPolicy>,
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    content_length: Option<u64>,
) -> Result<BodyPlan, IngressReject>;

pub fn transcode_buffered(
    codec: BodyCodec,
    method: &str,
    content_type: Option<&str>,
    raw: &[u8],
) -> Result<TranscodedBody, IngressReject>;

/// Case-insensitively removes `Content-Length`, `Content-Type`, `Transfer-Encoding`,
/// `Content-Encoding`, and `Expect`, then appends canonical `content-type` (if `Some`) and `content-length`.
pub fn apply_transcoded_headers(
    headers: &mut Vec<(Vec<u8>, Vec<u8>)>,
    content_type: Option<&'static str>,
    body_len: usize,
);

pub fn probe_sample(content_type: Option<&str>, raw_prefix: &[u8], total_len: u64) -> BodySample;
pub fn suggest_fix(samples: &[BodySample]) -> (Vec<BodyCodec>, bool);
```

**Normative `plan_body` + `transcode_buffered` Matrix:**

| Method | `policy` | Headers (`Content-Type` / `Encoding`) | Input `raw` | Verdict / Re-Encoded Output |
|---|---|---|---|---|
| `GET` / `HEAD` / `DELETE` / `OPTIONS` | any | any (identity encoding) | `b""` (`0` B) | `Ok(b"", None)` (`DELETE` with `> 0` B follows `POST` codec rules!) |
| `POST` / `PUT` / `PATCH` / `DELETE` | `None` or `BodyCodec::None` | any | `b""` -> `Ok`; `b"x"` -> `Err(413)` | `AllowEmptyOnly` rejects non-empty fixed or chunked bodies with `413` |
| any | `Json` or `Form` | `Content-Encoding: gzip` | any | `Err(400 InvalidPayload "content-encoding not supported")` |
| `POST` / `PUT` / `PATCH` | `BodyCodec::Json` | `application/json` | `b""` (`0` B) | `Err(400 InvalidPayload "empty json body")` |
| `POST` | `BodyCodec::Json` | `application/vnd.api+json; charset=UTF-8` | `b" { \"b\" : 1e0 , \"a\" : \"\\u00e9\" } "` | `Ok(b"{\"a\":\"\xc3\xa9\",\"b\":1.0}", Some("application/json"))` |
| `POST` | `BodyCodec::Json` | `application/json` | `b"{\"id\":18446744073709551616}"` | `Err(400 InvalidPayload "integer out of 64-bit range")` (`u64::MAX` passes; `u64::MAX + 1` rejected!) |
| `POST` | `BodyCodec::Json` | `application/json` | `b"{\"a\":1,\"a\":2}"` / BOM / depth `65` / trailing | `Err(400 InvalidPayload)` (depth `64` passes) |
| `POST` | `BodyCodec::Form` | `application/x-www-form-urlencoded; charset=UTF-8` | `b""` (`0` B) | `Ok(b"", Some("application/x-www-form-urlencoded"))` |
| `POST` | `BodyCodec::Form` | `application/x-www-form-urlencoded` | `b"b=hello+world&a=%41%3B1&b=2"` | `Ok(b"b=hello+world&a=A%3B1&b=2", Some("application/x-www-form-urlencoded"))` |
| `POST` | `BodyCodec::Form` | `charset=ibm037` or `b"a=%ZZ"` or `b"a=1%00"` or `b"a=%FF"` | any | `Err(400)` (`percent_encoding` hex + UTF-8 + NUL check) |
| any | `BodyCodec::Raw` | any | streamed | `BodyPlan::StreamPassthrough` (0 B buffered in scheduler) |

#### B. `crates/blastit-manifest` (`model.rs`, `parse.rs`, `validate.rs`, `edit.rs`)
- `Manifest` gains `app(&self) -> Option<&str>`, `preload(&self) -> &[String]`, and shared helper `pub fn is_query_covered_by_tables_or_groups(&self, route_index: usize, proxy: &ProxyName, normalized_sql: &str) -> bool` (reused by both `crates/blastit/src/audit_v1.rs` and `crates/blastit-py/src/audit.rs`).
- `Route` gains `with_ingress(mut self, ingress: Option<IngressPolicy>) -> Self` and `is_unrestricted_catch_all(&self) -> bool` (`methods().is_none() && path().iter().any(|p| p == ".*" || p == "^.*$")`).
- `edit.rs` (Rust-only structs with JSON-safe `Vec<ProposedGrant>`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct ProposedRoute { pub name: String, pub path: Vec<String> }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct ProposedGrant { pub route: String, pub proxy: String, pub queries: Vec<String> }

  #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
  pub struct ManifestEdits {
      pub routes: Vec<ProposedRoute>,
      pub grants: Vec<ProposedGrant>,
  }

  pub fn apply_edits(source: &str, edits: &ManifestEdits) -> Result<String, EditError>;
  pub fn write_atomic(path: &Path, content: &str) -> io::Result<()>;
  ```
  `apply_edits` locates the insertion index from the parsed `Manifest` (`first route where is_unrestricted_catch_all()`, matching both `path = ".*"` and `path = ['.*']`), edits line-by-line preserving comments, and re-parses to prove every untouched field is unchanged.

#### C. `crates/blastit-sqlpolicy` & `crates/blastit-events`
- `sqlpolicy`: `has_quoted_literal`, `redact_literals`, and `check_learnable` (`KNOWN_STATIC_SETUP_SQL` checked at step 2; `pg_query::parse` errors map to `LearnVerdict::Refused { redacted, reason: RefuseReason::Unparseable(err) }`; `redact_literals` replaces the entire literal span including `E`/`B`/`X`/`N`/`U&`/`$tag$` prefix with `…`). Also adds `pub fn is_introspection_setup_sql(normalized: &str) -> bool` matching `SET …`, `SHOW …`, `KNOWN_STATIC_SETUP_SQL`, and read-only `pg_catalog` / `current_schema()` / `to_regtype($?)` queries for `DynamicCheck` session-init replay.
- `events`: `Lifecycle` adds `pub fn with_build(self, build: impl Into<String>) -> Self` (set **only** when `stage1` constructs the `preload` `Boot` event; `None` on all other `Boot`/`Ready`/`Shutdown` instances, updating `json.rs`/`pretty.rs` round-trip tests; zero `port` field on any event).

#### D. `crates/blastit-http`, `crates/blastit-scheduler` & `crates/blastit-dbproxy`
1. **`blastit-http` early abort (`src/chan.rs`, `src/conn.rs`):** `Stream` adds `pending: Option<Frame>` (`try_peek_response(&mut self) -> bool` pulls non-`RespHead` frames like `Window` into credit and stores `Frame::RespHead` in `self.pending`; `Stream::recv` drains `self.pending` first so `relay()` never loses `RespHead`!). During `stream_fixed_body` / `stream_chunked_body` and inside `wait_for_credit`, if `try_peek_response()` is true, `Conn` stops reading the client socket, enters `relay()` with `force_connection_close = true` (appending `("Connection", "close")` to `RespHead`), and closes the socket.
2. **`blastit-scheduler`:** `Scheduler` owns `learn_fallback_busy: bool` and `learn_fallback_queue: VecDeque<u32>` (queued behind `cfg.queue_timeout`, `503` on overflow/timeout; `RouteInfo` remains a pure value). Both `Scheduler` (in `dev --learn`) and `_check_pytest.py` (in `check`) speak one shared JSON-line control protocol over `ctl_fd` / `ctl.sock` (`5s` read/write deadline, max `4096` B/line; on error/timeout `Scheduler` aborts the request with `502` and `_check_pytest.py` raises `RuntimeError` -> exit `3`):
   - `{"op":"push_routed","route":"catalog"}\n` -> `{"ok":true}\n`
   - `{"op":"push_unrouted","method":"GET","path":"/orders/42/"}\n` -> `{"ok":true}\n`
   - `{"op":"pop"}\n` -> `{"ok":true}\n`
   - `{"op":"set_db","proxy":"db","database":"test_shop"}\n` -> `{"ok":true}\n`
3. **`blastit-dbproxy`:**
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   #[serde(tag = "scope", rename_all = "snake_case")]
   pub enum ObsTarget {
       Routed { route: String },
       Unrouted { method: String, path: String },
   }

   #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   pub struct WireSqlObs {
       pub target: ObsTarget,
       pub proxy: String,
       pub kind: SqlKind,
       pub sql: String,      // always `redact_literals(&normalized)` (zero user-derived literals)
       pub is_tx: bool,
       pub allowed: bool,
       pub verdict: LearnVerdict,
   }
   ```
   `ListenerSpec.route` accepts `"*"` iff `mode != ListenerMode::Enforce`. `RoutePolicySpec` is populated solely from `Manifest::effective_grants` (`ListenerSpec` is the authority in `Enforce`; `route_policies` is the authority in `DynamicCheck`/`Learn` when `route == "*"`). Unknown route names in `push_routed` evaluate against an empty `Allowlist` (`allowed: false`).

#### E. `crates/blastit/src/{cli.rs, check.rs, audit_v1.rs, infer_routes.rs}`
1. **CLI Grammar (`crates/blastit/src/cli.rs`):**
   - `main(host: Host, argv: Vec<String>, py: Option<Python<'_>>) -> u8`:
   - If `argv` starts with `"dev"` -> parses `Command::Dev { host, port, learn }` (`-h`/`--help` prints dev help).
   - If `argv` starts with `"check"` or `"test"` -> splits on `"--"` first (everything after `"--"` goes verbatim to `pytest_args`, rejecting `--blastit-manifest`/`--blastit-live-server` with exit `2`), parses `--json`, `--learn`, `--dry-run`, `-h`/`--help` -> dispatches `check::run(&host, check_args)`.
   - Otherwise -> existing stage-0/stage-1/`--role=dbproxy` flag loop (`--stage=1` and `--role=*` stay 100% unchanged for `execve`; top-level `--help` lists `blastit`, `blastit dev`, `blastit check`, and `PRESETS`).
   - `dbproxy` binary resolution in `stage0` and `check::run`: `Host::dbproxy()` (`BLASTIT_EXEC` / wheel `blastit/_bin/blastit-dbproxy`), falling back in `cargo test` builds to `current_exe().with_file_name("blastit-dbproxy")`.
2. **`CheckObsV1` Schema (`schema_version == 1` enforced by `CheckObsV1::new` and validated in `evaluate`; malformed `http_obs.jsonl` aborts `check::run` with exit `2` naming the file and reason):**
   - `HttpObsLine { method: String, path: String, content_type: Option<String>, body_len: u64, sample: Option<BodySample> }`.
   - `audit_v1::evaluate` reuses `RoutingTable::route`, `Manifest::effective_grants`, `Allowlist::check`, and `Manifest::is_query_covered_by_tables_or_groups`.
3. **Complete 9-Kind Worked JSON Contract (`docs/ci.md`):**
   *(Below, `"catalog"`, `"search"`, `"db"`, `"analytics"`, `"session"` are concrete example names showing exact substituted strings.)*
   - `no_observations`: `{"kind":"no_observations","severity":"error","fix":{"action":"observe"}}` (fires when `routes.exercised == 0` or (`!proxies.is_empty()` && `statements == 0`); suppresses `unexercised_route` when `obs.http.is_empty()`).
   - `unrouted_request`: `{"kind":"unrouted_request","severity":"error","method":"GET","path":"/robots.txt","fix":{"action":"add_route"}}` (deduped by `(method, path)`).
   - `missing_grant` *(parameterized)*: `{"kind":"missing_grant","severity":"error","route":"catalog","proxy":"db","sql":"SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?","fix":{"action":"add_query","to":"[route.db].queries"}}`.
   - `missing_grant` *(connection setup — not one route's grant!)*: `{"kind":"missing_grant","severity":"error","route":"catalog","proxy":"db","sql":"SELECT set_config('TimeZone', $?, false)","fix":{"action":"add_group","to":"[group.connection.db].queries"}}` (it runs on whichever route connects first; the manifest attaches `groups = ["connection"]` at top level).
   - `missing_grant` *(redacted literal or tx-control — never `add_query`!)*: `{"kind":"missing_grant","severity":"error","route":"catalog","proxy":"db","sql":"SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"name\" = …","fix":{"action":"observe"}}` (or `"sql":"SAVEPOINT"` for tx-control).
   - `dead_grant` *(route)*: `{"kind":"dead_grant","severity":"error","route":"search","proxy":"db","sql":"SELECT count(*) FROM \"p\"","fix":{"action":"remove_query","to":"[route.db].queries","unsafe":true}}`.
   - `dead_grant` *(group)*: `{"kind":"dead_grant","severity":"error","route":"search","group":"session","proxy":"db","sql":"SELECT 1","fix":{"action":"remove_query","to":"[group.session.db].queries","unsafe":true}}`.
   - `unexercised_route`: `{"kind":"unexercised_route","severity":"error","route":"checkout","fix":{"action":"add_test"}}`.
   - `missing_ingress`: `{"kind":"missing_ingress","severity":"error","route":"upload","observed":["multipart/form-data"],"fix":{"action":"declare_ingress","codecs":["raw"],"allow_raw":true}}`.
   - `ingress_mismatch`: `{"kind":"ingress_mismatch","severity":"error","route":"checkout","declared":"json","observed":["application/x-www-form-urlencoded"],"fix":{"action":"fix_ingress","codecs":["form"]}}`.
   - `raw_ingress`: `{"kind":"raw_ingress","severity":"error","route":"upload","fix":{"action":"fix_ingress","to":"[route.ingress]","allow_raw":true}}`.
   - `proxy_unused`: `{"kind":"proxy_unused","severity":"error","proxy":"analytics","fix":{"action":"observe"}}`.
4. **13-Row `infer_routes` Fixture Table (`crates/blastit/src/infer_routes.rs`):**
   *(Name sanitizer: lowercase ASCII, map runs outside `[a-z0-9_-]` to `-`, trim **both leading and trailing** chars outside `[a-z0-9]`, fallback `"route"`, `"/"` -> `"root"`.)*
   1. `["/"]`, existing `["products"]` -> `("root", ["/"])` at EOF
   2. `["/products/"]`, existing `[]` -> `("products", ["/products/"])` at EOF
   3. `["/products/42/", "/products/99/"]`, existing `[]` -> `("products", ["/products/[0-9]+/"])` at EOF
   4. `["/products/", "/products/42/"]`, existing `[]` -> `("products", ["/products/", "/products/[0-9]+/"])` at EOF
   5. `["/orders/550e8400-e29b-41d4-a716-446655440000"]`, existing `[]` -> `("orders", ["/orders/[^/]+"])` at EOF
   6. `["/123/456"]`, existing `[]` -> `("route", ["/[0-9]+/[0-9]+"])` at EOF
   7. `["/api.v2/items+list/"]`, existing `[]` -> `("api-v2", ["/api\\.v2/items\\+list/"])` at EOF
   8. `["/_internal/ping"]`, existing `[]` -> `("internal", ["/_internal/ping"])` at EOF
   9. `["/café/1"]`, existing `[]` -> `("caf", ["/café/[0-9]+"])` at EOF *(trailing `-` trimmed!)*
   10. `["/products/café/"]`, existing `[]` -> `("products", ["/products/café/"])` at EOF
   11. `["/products/42/"]`, existing `["products"]` -> `("products-2", ["/products/[0-9]+/"])` at EOF
   12. `["/robots.txt"]`, existing `["products", "fallback"(path=['.*'])]` -> `("robots-txt", ["/robots\\.txt"])` before `"fallback"`
   13. `["/robots.txt"]`, existing `["products", "fallback"(path=".*")]` -> `("robots-txt", ["/robots\\.txt"])` before `"fallback"` *(scalar `path = ".*"`!)*

### 4.3 Resource & Memory Budgets

| Boundary | Hard Ceiling | Enforcement Point |
|---|---|---|
| HTTP request head | `BLASTIT_MAX_HEADER_BYTES` (`64 KiB` default) | `blastit-http` (`431` before scheduler) |
| HTTP request body (`http`) | `BLASTIT_MAX_BODY_BYTES` (`100 MiB` default) | `blastit-http` (`Content-Length` upfront `413` + `Dechunker` cap) |
| Scheduler ingress buffer | `BLASTIT_MAX_BODY_BYTES` (`Json`/`Form` only; **`0` B for `Raw`**) | `blastit-scheduler` (`BodyPlan::StreamPassthrough` bypasses buffer) |
| Check-mode body sample | `MAX_CHECK_SAMPLE_BYTES = 64 KiB` (`wsgi.input` spooled/replayed) | `python/blastit/_check_pytest.py` + `blastit-ingress::probe_sample` |
| JSON AST nesting & ints | `MAX_JSON_DEPTH = 64`, exact `i64`/`u64`/finite `f64` | `blastit-ingress` visitor (`400`) |
| Structured log event line | `MAX_EVENT_BYTES = 2048` | `blastit-events` |
| `dbproxy` `ctl.sock` JSON line | `4096` bytes, `5s` I/O timeout | `blastit-dbproxy` & `blastit-scheduler` / `_check_pytest.py` |

---

## 5. Security & CVE Analysis

1. **Secret Exfiltration via Unparameterized SQL (`S1`):** `redact_literals` scrubs all 8 PG literal syntaxes (including prefix) to `…` inside `dbproxy` before any `SqlDenied` or `WireSqlObs` is emitted; `Manifest::validate` rejects both `…` and inline user literals (`LiteralQuery`); `--learn` and `fold` refuse literal-bearing queries; `check --json` never emits `add_query` for redacted SQL.
2. **Request-Time RCE, Credential Barrier & Hermetic Check:** Real DB credentials stay in `dbproxy` (`Zeroizing`, empty `envp`, non-dumpable). In `blastit check`, non-Django `pytest` runs receive `swapped_env` (`socket_dir = <runtime_dir>/pg`) with zero real credentials in `os.environ`, and `-p no:blastit` blocks legacy `0.1.0` plugin interference. Views remain locked to `AF_UNIX` by `seccomp` unless `egress = true`.
3. **HTTP Smuggling & Parser Differentials (`S4`):** `blastit-http` rejects `CL`+`TE` ambiguity (`400`) and aborts uploads immediately on early `413`/`400` (`Connection: close`); `blastit-ingress` rejects non-identity `Content-Encoding`, non-UTF-8 charsets, duplicate JSON keys, out-of-range 64-bit integers, malformed `%XX` escapes, and form NUL bytes (`400`), re-serializes canonically, and strips `Transfer-Encoding`/`Content-Encoding`/`Expect`/duplicate `Content-Length` via `apply_transcoded_headers`.
4. **Dev Exposure & Fail-Closed Production (`S3`, `S6`, `S10`):** `blastit dev` binds `127.0.0.1` and refuses non-loopback `--host` without `BLASTIT_DEV_EXPOSE=1`; production `blastit` has zero single-UID fallback and aborts on missing capabilities or `BLASTIT_SECCOMP=0` under any preset.

---

## 6. Alternatives Considered

1. **`--manifest` CLI flags or upward parent search:** Rejected — monorepo ancestor policy hazard and contradicts `docs/reference/cli.md`.
2. **Per-framework ORM monkeypatches or Python-launched `dbproxy`:** Rejected — Rust `check::run` keeps real credentials out of Python for WSGI suites and reuses `blastit-dbproxy` on the wire.
3. **Whole-body `evaluate_body` including `Raw`, or `serde_json/arbitrary_precision`:** Rejected — `plan_body` keeps `Raw` zero-buffered (`R16`), and rejecting >64-bit integer overflow avoids workspace-wide `serde_json` feature unification while preventing silent `f64` rounding.
4. **Auto-synthesizing `tables` grants or `transaction_control = true` in `--learn`:** Rejected — `--learn` writes only `routes` and exact `queries` (`docs/ci.md`, `docs/frameworks/flask.md`).

---

## 7. Validation & Critical Tests Plan

### 7.1 Execution Tiers, Harness Isolation, and Offline Recipe
- **Zero-Skip Enforcement (`R42`):** `Makefile` `test-critical` checks `crates/*/tests/critical_*.rs` and `python/tests/critical/test_*.py` with an attribute/call regex (`#\[\s*ignore`, `\bneeds_root\s*\(`, `pytest\.mark\.skip`, `pytest\.skip\s*\(`, `importorskip\s*\(`, `unittest\.skip`) so comments never false-positive and zero critical tests skip.
- **Harness Isolation:** Env-mutating tests in `critical_config_and_presets.rs` hold a global `Mutex` (or run with `--test-threads=1`); `MockPgServer` binds `127.0.0.1:0` and reads `local_addr().port()`; `seccomp` critical assertions run a preflight filter check with an actionable kernel error and accept `EPERM | ENOSYS`.
- **OfflineStaging Recipe (`R42`):** `Makefile` `test-critical` runs `cargo build --locked -p blastit-py -p blastit-dbproxy`, copies `target/debug/lib_blastit.so` to `<stage>/blastit/_blastit$(EXT_SUFFIX)` (with `RUST_ENV`'s `LD_LIBRARY_PATH` + `PYTHONHOME`) and `target/debug/blastit-dbproxy` to `<stage>/blastit/_bin/blastit-dbproxy`, and runs `CARGO_NET_OFFLINE=1 UV_OFFLINE=1 PYTHONPATH="<stage>:python" uv run --no-project python -m unittest discover -s python/tests/critical -p 'test_*.py'`.
- **Docs Fence Suite (`crates/blastit-manifest/tests/docs_fences.rs`, standard tier):** Placed in standard (non-protected) tests so `MA.3` can update `docs/**` freely without `critical_guard` conflicts; asserts the exact 6 complete `title="blastit.toml"` files (`README.md`, `docs/index.md`, `docs/getting-started.md`, `docs/local.md`, `docs/frameworks/django.md`, `docs/frameworks/flask.md`, `count == 6`) parse cleanly plus a negative invalid-regex canary.
- **Mutation Gate:** `make mutants-rust PKG=blastit-ingress`, `PKG=blastit-sqlpolicy`, and `PKG=blastit-manifest` target **≥ 75% killed** at the `MA` gate.

### 7.2 Requirement -> Enforcement Crate & Positive/Negative Critical Test Matrix

| Req IDs | Enforcing Crate / Module | Protected Critical Suite (`make check`) | Required Positive (Allow) **and** Negative (Deny / Adversarial) Twins |
|---|---|---|---|
| `R1..R3`, `R6`, `R9..R10`, `R18`, `R23` | `crates/blastit/{cli,config}` + `blastit-events` | `crates/blastit/tests/critical_config_and_presets.rs` | *(+)* `./blastit.toml` CWD load, both presets' 4 values + `preset_overridden` when `BLASTIT_*` (incl. `""`) overrides `--preset`, `build == CARGO_PKG_VERSION` on `preload` `boot` only (`None` on `ready`/`shutdown`).<br/>*(−)* Table-driven `cli_matrix` covering all R1–R3 rejections (`dev --manifest`, `dev --preset`, `dev --port 70000`, `dev --host 999.9.9.9`, `check --manifest`, `check --preset`, `check --dry-run`, `test --preset`, `blastit --stage=1 --preset x`, `check -- --blastit-manifest x`) verifying exact exit code (`1` vs `2`), `stderr` text, and `stdout == b""`; `BLASTIT_SECCOMP=0` + preset aborts (`exit 1`). |
| `R7..R8`, `R11`, `R24` (manifest), `R29` (edit) | `crates/blastit-manifest` | `crates/blastit-manifest/tests/critical_manifest_011.rs` (+ standard `docs_fences.rs`) | *(+)* `app`, string/array `preload`, route `groups`, `[route.ingress]`, `KNOWN_STATIC_SETUP_SQL` valid, `edit::apply_edits` preserves comments + inserts before both `path = ".*"` and `path = ['.*']` + `write_atomic` `.blastit.toml.<pid>.tmp` cleanup.<br/>*(−)* Reserved key `groups`/`app`/`preload`/`version` inside `[table]` emits exact message; `allow_raw` on `json` -> `UnknownKey`; `…` and `'secret'` in `queries` -> `RedactedQueryPlaceholder` / `LiteralQuery`; `RawIngress` + `EgressRoute` warnings emitted. |
| `R14..R16` | `crates/blastit-ingress` | `crates/blastit-ingress/tests/critical_ingress.rs` | *(+)* Every `Ok` row of §4.2 A (`application/vnd.api+json; charset=UTF-8`, `u64::MAX`, depth `64`, sorted keys, `Form` `;` -> `%3B`, `Raw` `BodyPlan::StreamPassthrough`, `apply_transcoded_headers` single `content-length`).<br/>*(−)* `u64::MAX + 1`, depth `65`, duplicate JSON keys, BOM, `Content-Encoding: gzip`, `DELETE` with invalid body, `Form` `%ZZ` / `%00` / `%FF` / `charset=ibm037`, and `apply_transcoded_headers` stripping `Transfer-Encoding: chunked`. |
| `R12..R13`, `R16` | `crates/blastit-http` + `blastit-scheduler` | `crates/blastit-http/tests/critical_early_abort.rs` & `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` | *(+)* Two-phase `Raw` stream delivers chunk 1 to view before chunk 2 is sent; chunked body `== cap` passes; valid chunked `json` reaches view with `HTTP_TRANSFER_ENCODING` absent from `environ`; `learn_mode` queues concurrent unrouted requests behind `learn_fallback_busy`.<br/>*(−)* `CL: cap + 1` -> upfront `413`; chunked `cap + 1` -> `413` + `Connection: close`; scripted `10 MiB` upload panicking on post-`RespHead(413)` read closes immediately; non-empty body on ingress-less / `none` route -> `413`. |
| `R24..R25` (sql) | `crates/blastit-sqlpolicy` | `crates/blastit-sqlpolicy/tests/critical_literals.rs` | *(+)* `"ident"`, `U&"ident"`, `$1`->`$?`, `KNOWN_STATIC_SETUP_SQL`, `pg_catalog.version()`, `SHOW` -> `LearnVerdict::Query`.<br/>*(−)* Exact pinned redaction (`SELECT E'\n'`, `SELECT B'1'`, `SELECT U&'\0041'`, `SELECT $t$x$t$` -> `SELECT …`), unparseable SQL -> `RefuseReason::Unparseable`, multi-statement, `COPY … PROGRAM`, and all `DANGEROUS_LEARN_FUNCTIONS` refused. |
| `R26` (db), `R31` | `crates/blastit-dbproxy` | `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` | *(+)* Empty stack ignores fixture `INSERT` (`0` `obs_fd` lines) while recording `SET TIME ZONE 'UTC'` + `select pg_catalog.version()` and replaying both once on first `push_routed`; `DynamicCheck` returns `Allow`; `set_db` switches DB.<br/>*(−)* `Enforce` returns `42501`, scrubs `'s3cr3t'` across entire `SqlDenied` and `obs_fd` byte streams, and rejects `uid == 0` when `!allow_root_listener`. |
| `R17`, `R19..R22`, `R29` (`dev`) | `crates/blastit` | `crates/blastit/tests/critical_dev_and_fail_closed.rs` | *(+)* Unprivileged `blastit dev` boots 5 roles (`privilege: "unprivileged-dev"`), prints exact 3-line banner (`1 route` & `2 routes`, plus `EXPOSED` with `BLASTIT_DEV_EXPOSE=1`), strips real `DATABASE_URL` + `BLASTIT_*` + `BLASTIT_ENV_STRIP` names, and `dev --learn` writes routes/queries on `SIGTERM`.<br/>*(−)* Unprivileged production `blastit` exits `1`; ungranted route gets `ENOENT`; `egress = false` view gets `EPERM|ENOSYS` on `socket(AF_INET)`; `--host 0.0.0.0` without `BLASTIT_DEV_EXPOSE=1` exits `1`. |
| `R27..R28`, `R32..R38` | `crates/blastit/{audit_v1,infer_routes}` + `blastit-py` | `crates/blastit/tests/critical_check_engine.rs` & `crates/blastit-py/tests/critical_check_schema.rs` | *(+)* All 13 `infer_routes` rows; `R27` golden test where unrouted `POST /orders/42/` runs 2 queries -> `plan_learn` emits `[[route]] name = "orders"` with those exact 2 `queries`; unrouted-only suite exits `0` on `--learn`; every `add_query`/`add_group` emitted by golden fixtures re-parses cleanly under `Manifest::parse_and_validate`; exact §1.3 human lines verified byte-for-byte.<br/>*(−)* All 9 finding kinds + interaction counts; the `add_group` oracle lives in `crates/blastit/tests/critical_check_engine.rs` (`MB.1.13`): connection-setup SQL (`SELECT set_config('TimeZone', $?, false)`, `SET …`, `SHOW …`) emits exactly `{"action": "add_group", "to": "[group.connection.db].queries"}`, while `SELECT 1` and ordinary statements keep `{"action": "add_query", "to": "[route.db].queries"}`; redacted `…` and `SAVEPOINT` denials emit `"action": "observe"` (never `add_query`, never a group hint); `--learn` with inline literal or `0` requests sets `"wrote": false` and exits `1`. |
| `R4..R5`, `R30`, `R39..R42` | `crates/blastit/check` + `python/blastit/*` | `python/tests/critical/test_fold_literals_and_atomic.py` & `python/tests/critical/test_check_contract.py` (+ `python/tests/test_check.py` in PG CI) | *(+)* Mini-runner `-m pytest` loads `-p blastit._check_pytest` (while `-p no:blastit` blocks `pyproject.toml` `blastit_manifest` `0.1.0` plugin!), runs raw-WSGI + Flask-like `.wsgi_app` (`ContextVar` single-count, `os.environ` free of real DB password!) through real `blastit-dbproxy` + `MockPgServer` -> `exit 0`, `requests == 1`, `findings == []`, 1 JSON doc despite `print()`/`os.write(1, b"noise")`; Python `HttpObsLine` -> `_blastit.check_audit` conformance; `pyproject.toml` `dependencies == []`; `blastit_live_server` uses `blastit dev` non-root.<br/>*(−)* Bad manifest or corrupt `http_obs.jsonl` -> `exit 2`, `stdout == b""`; `pytest` exit `1` and `5` -> `exit 3`, `stdout == b""`; `fold` refuses `'secret'`/`…` and cleans `.tmp` on error. |

---

## 8. Implementation Plan (12 Leaves, ≤ 450 Lines Each)

All 12 leaves stay inside `MA.2`'s `paths` (`["crates/**", "python/**", "Makefile"]`). Parallel groups (`{L2, L3}`, `{L4, L5}`, `{L6, L7, L8}`) have **100% disjoint `paths`**, and Leaf 1 lands struct shims + `Cargo.lock` upfront so every leaf compiles workspace-wide.

1. **`workspace-seams-and-cargo-wiring` (~180 lines, sequential)**
   - **Paths:** `["Cargo.toml", "Cargo.lock", "crates/*/Cargo.toml", "crates/blastit-ingress/src/lib.rs", "crates/blastit-env/src/**", "crates/blastit/src/stage0.rs", "crates/blastit/src/stage1.rs", "crates/blastit-scheduler/src/config.rs", "crates/blastit-scheduler/src/route.rs", "crates/blastit-dbproxy/src/config.rs"]`
   - **Scope:** Create `blastit-ingress` skeleton (`BodyCodec`, `IngressPolicy`); wire §4.1 `Cargo.toml` matrix (incl. `percent-encoding = "2.3.2"`) + `Cargo.lock`; add `blastit_env::build_with_socket_dir`; add `ListenerSpec::enforcing`, `DbproxyConfig::enforcing`, `RouteInfo::with_ingress`, and `..SchedulerConfig::default()` at `stage0.rs`/`stage1.rs`.
   - **Critical test:** *(foundation wiring)* | **Acceptance:** `CARGO_NET_OFFLINE=1 cargo test --workspace --all-targets --locked && make check`

2. **`ingress-codecs-and-canonical-reencode` (~360 lines, after L1)**
   - **Paths:** `["crates/blastit-ingress/src/**", "crates/blastit-ingress/tests/**"]`
   - **Scope:** Implement `BodyPlan`, `BodySample`, `parse_mime`, `plan_body`, `transcode_buffered` (§4.2 A matrix, 64-bit integer range check, `percent-encoding` validation), `apply_transcoded_headers`, `probe_sample`, and `suggest_fix` (`R14..R16`).
   - **Critical test:** `crates/blastit-ingress/tests/critical_ingress.rs` | **Acceptance:** `cargo test -p blastit-ingress --test critical_ingress --locked && make check`

3. **`sqlpolicy-literal-redaction-and-learnable` (~300 lines, after L1 — parallel with L2)**
   - **Paths:** `["crates/blastit-sqlpolicy/src/**", "crates/blastit-sqlpolicy/tests/**"]`
   - **Scope:** Extend `lexer.rs` (`N'…'`, `U&'…'`, `$tag$…$tag$`); implement `has_quoted_literal`, `redact_literals`, `check_learnable` (`RefuseReason::Unparseable`, `KNOWN_STATIC_SETUP_SQL`, `DANGEROUS_LEARN_FUNCTIONS`), and `is_introspection_setup_sql` (`R24`, `R25`, `R31`).
   - **Critical test:** `crates/blastit-sqlpolicy/tests/critical_literals.rs` | **Acceptance:** `cargo test -p blastit-sqlpolicy --test critical_literals --locked && make check`

4. **`manifest-011-schema-and-warnings` (~320 lines, after L2 + L3)**
   - **Paths:** `["crates/blastit-manifest/src/lib.rs", "crates/blastit-manifest/src/model.rs", "crates/blastit-manifest/src/parse.rs", "crates/blastit-manifest/src/validate.rs", "crates/blastit-manifest/src/error.rs", "crates/blastit-manifest/tests/**"]`
   - **Scope:** Add `app`, `preload`, `[route.ingress]`, reserved-key-in-table guard (`R8`), `LiteralQuery`/`RedactedQueryPlaceholder`, `ManifestWarning::{RawIngress, EgressRoute}`, `is_unrestricted_catch_all`, `is_query_covered_by_tables_or_groups`, and standard `docs_fences.rs` (`R7`, `R8`, `R11`, `R24`).
   - **Critical test:** `crates/blastit-manifest/tests/critical_manifest_011.rs` | **Acceptance:** `cargo test -p blastit-manifest --test critical_manifest_011 --locked && cargo test -p blastit-manifest --test docs_fences --locked && make check`

5. **`manifest-surgical-editor-and-atomic-write` (~340 lines, after L4)**
   - **Paths:** `["crates/blastit-manifest/src/lib.rs", "crates/blastit-manifest/src/edit.rs", "crates/blastit-manifest/tests/critical_manifest_edit.rs"]`
   - **Scope:** Implement pure-Rust comment-preserving `edit::{apply_edits, write_atomic}` (inserting before `is_unrestricted_catch_all()` for both `path = ".*"` and `path = ['.*']`, re-parse verification, `.{name}.{pid}.tmp` + `fsync` + `rename` + dir `fsync`) (`R28`, `R29`).
   - **Critical test:** `crates/blastit-manifest/tests/critical_manifest_edit.rs` | **Acceptance:** `cargo test -p blastit-manifest --test critical_manifest_edit --locked && make check`

6. **`http-early-abort-and-scheduler-ingress` (~420 lines, after L4)**
   - **Paths:** `["crates/blastit-http/src/**", "crates/blastit-http/tests/**", "crates/blastit-scheduler/src/**", "crates/blastit-scheduler/tests/**"]`
   - **Scope:** Add `Stream.pending` early-response upload abort + `Connection: close` in `blastit-http` (`R13`); wire `plan_body` / `transcode_buffered` / `apply_transcoded_headers` + `learn_fallback_busy` + JSON-line `ctl_fd` `push_unrouted`/`pop` in `blastit-scheduler` (`R12`, `R14..R16`, `R26`).
   - **Critical tests:** `crates/blastit-http/tests/critical_early_abort.rs`, `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` | **Acceptance:** `cargo test -p blastit-http --test critical_early_abort --locked && cargo test -p blastit-scheduler --test critical_ingress_enforcement --locked && make check`

7. **`dbproxy-learn-redaction-and-dynamic-check` (~420 lines, after L4 — parallel with L6)**
   - **Paths:** `["crates/blastit-dbproxy/src/**", "crates/blastit-dbproxy/tests/**"]`
   - **Scope:** Redact `SqlDenied` via `redact_literals`; implement `ListenerMode::{Enforce, Learn, DynamicCheck}`, `ObsTarget`, `WireSqlObs` (`SqlKind`), `allow_root_listener`, JSON-line `check_ctl_fd` (`push_routed`, `push_unrouted`, `pop`, `set_db` with `5s` timeout), and `is_introspection_setup_sql` replay (`R24`, `R26`, `R31`).
   - **Critical test:** `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` | **Acceptance:** `cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked && make check`

8. **`python-ruff-clean-makefile-and-fold` (~350 lines, after L3 — parallel with L6 & L7)**
   - **Paths:** `["Makefile", "python/blastit/__main__.py", "python/blastit/fold.py", "python/blastit/logs.py", "python/blastit/pytest_plugin.py", "python/tests/test_fold.py", "python/tests/test_logs.py", "python/tests/test_plugin_helpers.py", "python/tests/test_pytest_plugin.py", "python/tests/test_live_server.py", "python/tests/test_wire_capture.py", "python/tests/project/**", "python/tests/live_project/**", "python/tests/packaged_project/**", "python/tests/critical/**"]`
   - **Scope:** Fix all 72 `ruff check` & 12 `ruff format` issues across `python/`, gate `python tools` in `Makefile` `py-lint`/`py-format`, wire regex skip-guard + `_blastit<EXT_SUFFIX>`/`blastit-dbproxy` staging + offline `unittest discover` in `Makefile` `test-critical`, add `R40` `dependencies == []` check, and harden `fold.py` for `S1` literal refusal + `.{name}.{pid}.tmp` atomic write (`R25`, `R29`, `R40..R42`).
   - **Critical test:** `python/tests/critical/test_fold_literals_and_atomic.py` | **Acceptance:** `CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check`

9. **`cli-config-discovery-and-presets` (~380 lines, after L5 + L6 + L7)**
   - **Paths:** `["crates/blastit-events/src/**", "crates/blastit-events/tests/**", "crates/blastit/src/lib.rs", "crates/blastit/src/cli.rs", "crates/blastit/src/config.rs", "crates/blastit/tests/critical_config_and_presets.rs"]`
   - **Scope:** Add `privilege: "unprivileged-dev"` and `preload`-only `build` field in `blastit-events` (`R22`, `R23`); implement `Command::{Serve, Dev, Check}` parser (`cli.rs`), `./blastit.toml` CWD-only discovery, `app`/`preload` manifest fallback + empty-var precedence, `dev`/`check` `pretty` default, and `BLASTIT_SECCOMP=0` preset abort + `Warning::SeccompDisabled` (`R1..R3`, `R6`, `R9`, `R10`, `R18`).
   - **Critical test:** `crates/blastit/tests/critical_config_and_presets.rs` | **Acceptance:** `cargo test -p blastit --test critical_config_and_presets --locked && make check`

10. **`supervisor-unprivileged-dev-and-learn` (~450 lines, after L9)**
    - **Paths:** `["crates/blastit/src/lib.rs", "crates/blastit/src/stage0.rs", "crates/blastit/src/stage1.rs", "crates/blastit/src/child.rs", "crates/blastit/src/zygote.rs", "crates/blastit/src/supervise.rs", "crates/blastit/src/dev_learn.rs", "crates/blastit/tests/**"]`
    - **Scope:** Implement `ExecutionMode::Dev` (ephemeral `0700` `<root>/views/<idx>` dirs, stage-0 `execve` secret swap, unprivileged `seccomp`, no `RLIMIT_NPROC` lowering, 3-line banner, `BLASTIT_DEV_EXPOSE=1` guard, `dev --learn` `obs_fd` collector + `blastit_manifest::edit::write_atomic`), `crates/blastit/tests/common/mock_pg.rs` (`MockPgServer`), and fail-closed + `dev` e2e critical suite (`R17`, `R19..R22`, `R29`).
    - **Critical test:** `crates/blastit/tests/critical_dev_and_fail_closed.rs` | **Acceptance:** `cargo build --locked -p blastit-dbproxy && cargo test -p blastit --test critical_dev_and_fail_closed --locked && CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check`

11. **`check-audit-and-learn-engine` (~400 lines, after L10)**
    - **Paths:** `["crates/blastit/src/lib.rs", "crates/blastit/src/audit_v1.rs", "crates/blastit/src/infer_routes.rs", "crates/blastit/tests/critical_check_engine.rs", "crates/blastit-py/src/**", "crates/blastit-py/tests/**"]`
    - **Scope:** Implement `CheckObsV1` (`schema_version == 1`), 13-row `infer_routes`, 9-kind `audit_v1::evaluate` (bootable-only `add_query`, `observe` on redacted/tx denials, exact §1.3 9-row human formatter), `plan_learn` (`ObsTarget::Unrouted` -> inferred route query mapping), `_blastit.{check_audit, plan_learn, probe_ingress}`, and shared `is_query_covered_by_tables_or_groups` wiring in `crates/blastit-py/src/audit.rs` (`R27..R28`, `R32..R38`).
    - **Critical tests:** `crates/blastit/tests/critical_check_engine.rs`, `crates/blastit-py/tests/critical_check_schema.rs` | **Acceptance:** `cargo test -p blastit --test critical_check_engine --locked && cargo test -p blastit-py --test critical_check_schema --locked && make check`

12. **`check-orchestrator-wsgi-gate-and-python-tests` (~420 lines, after L8 + L11)**
    - **Paths:** `["crates/blastit/src/lib.rs", "crates/blastit/src/cli.rs", "crates/blastit/src/check.rs", "python/blastit/_check_pytest.py", "python/blastit/pytest_plugin.py", "python/tests/test_check.py", "python/tests/test_live_server.py", "python/tests/critical/**"]`
    - **Scope:** Implement `crates/blastit/src/check.rs` (`-p no:blastit -p blastit._check_pytest`, fd-1 -> fd-2 redirection, non-Django secret-free `swapped_env`, exit `0/1/2/3`), `python/blastit/_check_pytest.py` (`ContextVar` `BlastitWsgiGate`, `64 KiB` sample cap, JSON-line `ctl.sock`), unprivileged `blastit dev` path in `blastit_live_server`, `python/tests/test_check.py` (real pytest/PG suite), and `python/tests/critical/test_check_contract.py` (mini-runner + stub-pytest + `-p no:blastit` + adversarial stdout-purity suites) (`R3..R5`, `R30`, `R31`, `R39`).
    - **Critical test:** `python/tests/critical/test_check_contract.py` | **Acceptance:** `cargo build --locked -p blastit-dbproxy -p blastit-py && CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check`

---

## 9. Review Resolution

### 9.1 Pass 1 Resolutions (`implementability`, `testability`, `consistency`)
All Pass-1 findings (`I-B1..I-B5`, `I-M1..I-M11`, `I-m1..I-m9`, `T-B1..T-B5`, `T-M1..T-M12`, `T-m1..T-m8`, `C-B1..C-B2`, `C-M1..C-M12`, `C-m1..C-m12`) were incorporated and audited in Pass 2:
- Closed in Pass 1 and verified in Pass 2: `I-B1..I-B4` (Leaf 1 constructor shims, `ListenerMode`/`RoutePolicySpec`, Rust `check` orchestrator, unified `blastit-manifest::edit`), `T-B2..T-B5` (offline staging recipe, `CheckObsV1` schema, enforcing-crate mapping, `DynamicCheck` always-`Allow`), `C-B2` (`fold` exit `0`, all-or-nothing `check --learn`), `C-M1..C-M9`, `C-M11` (`LiteralQuery`).

### 9.2 Pass 2 Resolutions (`implementability` `I2-*`, `testability` `T2-*`, `consistency` `C2-*`)

| ID | Severity | Resolution |
|---|---|---|
| `I2-B1` | BLOCKER | `accepted -> R16, §4.2 A, §4.3, ADR-0006`: Replaced whole-body `evaluate_body` with `plan_body` (`BodyPlan::StreamPassthrough` for `Raw` -> `0` B scheduler buffer) + `transcode_buffered` (`Json`/`Form` only), with explicit §4.3 budget row. |
| `I2-B2` | BLOCKER | `accepted -> R3, R30, §4.2 E, §7.2, ADR-0004`: `check.rs` spawns `pytest` with `-p no:blastit -p blastit._check_pytest` and rejects forwarded `--blastit-manifest`/`--blastit-live-server` (`exit 2`), verified against `pyproject.toml` `blastit_manifest` in `test_check_contract.py`. |
| `I2-B3` | BLOCKER | `accepted -> R15, §4.1, §4.2 A, ADR-0006`: Added `percent-encoding = "2.3.2"` to `blastit-ingress/Cargo.toml` in Leaf 1; `Form` validates `%XX` hex + UTF-8 via `percent_encoding` and uses `url` only to serialize. |
| `I2-B4` | BLOCKER | `accepted -> R14, R15, §4.2 A, ADR-0006`: Added `apply_transcoded_headers` stripping `Transfer-Encoding`, `Content-Encoding`, `Expect`, `Content-Length`, and `Content-Type` (and rejecting non-identity `Content-Encoding` with `400`), tested on chunked `POST`. |
| `I2-B5` | BLOCKER | `accepted -> §8`: Split `MA.2` further into **12 crisp leaves (~180–450 lines each)** along `sqlpolicy`, `manifest` schema vs `edit.rs`, `cli/config` vs `stage0/dev`, and `audit_v1` vs `check.rs`/`_check_pytest.py`. |
| `I2-M1` | MAJOR | `accepted -> R14, §4.2 A, ADR-0006`: JSON visitor rejects integer lexemes outside `[i64::MIN, u64::MAX]` with `400` instead of silently rounding `u64::MAX + 1` to `f64`. |
| `I2-M2` | MAJOR | `accepted -> §4.2 D.2`: Moved `learn_fallback_busy` + `learn_fallback_queue` onto `Scheduler` (keeping `RouteInfo` a pure value), queued behind `cfg.queue_timeout`. |
| `I2-M3` | MAJOR | `accepted -> §4.2 D.2, §4.3`: Unified `ctl.sock` on a single JSON-line protocol (`push_routed`, `push_unrouted`, `pop`, `set_db`) with `4096` B max line, `5s` deadline, and `502`/exit-`3` error mapping. |
| `I2-M4` | MAJOR | `accepted -> §4.2 D.3`: Allowed `ListenerSpec.route == "*"` iff `mode != Enforce` and derived `RoutePolicySpec` exclusively from `Manifest::effective_grants`. |
| `I2-M5` | MAJOR | `accepted -> R34, §4.2 B/E`: Extracted `Manifest::is_query_covered_by_tables_or_groups` in `blastit-manifest` shared by both `audit_v1` and `blastit-py/src/audit.rs`. |
| `I2-M6` | MAJOR | `accepted -> R23, §4.2 C`: Dropped `port` from events (`C2-M1`) and scoped `Lifecycle.build` strictly to `preload` `Boot` (`None` elsewhere). |
| `I2-M7` | MAJOR | `accepted -> R8`: Defined `R8` directly in terms of `parse.rs`'s `toml::Table` walk (reserved key inside nested table) without requiring `toml_edit`/spans. |
| `I2-M8` | MAJOR | `accepted -> R36, §4.2 A, §4.3`: Capped `check` body probing at `MAX_CHECK_SAMPLE_BYTES = 64 KiB` (`truncated: true` falls back to `raw` + `allow_raw`). |
| `I2-M9` | MAJOR | `accepted -> R13, §4.2 D.1`: Specified `Stream.pending: Option<Frame>` on `blastit_http::chan::Stream` + `("Connection", "close")` header append so `RespHead` is never lost before `relay()`. |
| `I2-M10` | MAJOR | `accepted -> §4.2 E.1`: Specified exact `argv` dispatch in `cli.rs` preserving `--stage=1`/`--role=*` and `Host::dbproxy()` + sibling `current_exe()` lookup. |
| `I2-m1..m8` | MINOR | `accepted -> §4.1, §4.2 A/B/D/E, §7.1`: Used `Vec<ProposedGrant>` (`m1`), `CheckObsV1::new` enforcing `1` (`m2`), `SqlKind` on `WireSqlObs` (`m3`), 64 B cap on `BodySample.mime` (`m4`), noted zero changes to `frame`/`sys`/`view` (`m5`), added `EXT_SUFFIX` + `RUST_ENV` staging details (`m6`), verified `docs/ci.md:59` already contains `schema_version: 1` (`m7`), and documented `parse_mime` tuple (`m8`). |
| `T2-B1` / `C2-B1` | BLOCKER | `accepted -> §2, R33, §4.2 E.3, §7.1, §7.2, ADR-0003, ADR-0004`: `missing_grant` emits `"action": "observe"` (never `add_query`) for redacted `…` or tx-control SQL, with a critical oracle proving every emitted `add_query` boots; moved `docs_fences.rs` (`count == 6`) to standard tests so `MA.3` isn't blocked by `critical_guard`, and scheduled the `manifest.md:48` / `local.md:114` prose edits in `MA.3` (which owns `docs/**`). |
| `T2-B2` | BLOCKER | `accepted -> §7.2, §8 Leaf 12`: Split `test_check_contract.py` (offline stub-pytest + 25-line mini-runner actually loading `-p blastit._check_pytest` against WSGI + `MockPgServer` -> `exit 0`) and `python/tests/test_check.py` (real `pytest` + PostgreSQL in `pytest-plugin` CI & `MA.3`). |
| `T2-B3` / `C2-M2` | BLOCKER | `accepted -> R27, R30, §4.2 D.2/D.3, §7.2`: Replaced `__learn__`/`__unrouted__:` with typed `ObsTarget::{Routed, Unrouted}` over JSON-line `ctl.sock`, defined `plan_learn` unrouted->inferred-route mapping, suppressed route-less `missing_grant` on `Unrouted`, and added `R27`'s golden critical test. |
| `T2-M1..M11` | MAJOR | `accepted -> §1.3, §4.2 A/E, §7.1, §7.2, §8`: Added banner/human/`learn:` byte assertions (`T2-M1`), trailing `-` trim + `/products/café/` row (`T2-M2`), concrete substituted JSON examples (`T2-M3`), mutex/`MockPgServer` port-0/`EPERM\|ENOSYS` harness rules (`T2-M4`), `cargo build -p blastit-dbproxy -p blastit-py` prefixes (`T2-M5`), directory globs `tests/**` (`T2-M6`), attribute-specific skip regex (`T2-M7`), chunked `cap`/`cap+1` tests (`T2-M8`), `cli_matrix` table (`T2-M9`), `R40` `pyproject.toml` test (`T2-M10`), and Python->Rust `CheckObsV1` conformance + corrupt-JSONL `exit 2` (`T2-M11`). |
| `C2-M1` | MAJOR | `accepted -> §1.1, R2, R22, R23, §4.2 C, ADR-0001`: Dropped `port` from `ready` and dropped `--port 0`, preserving `ARCHITECTURE.md:1399`'s "a port is never a log field" invariant. |
| `C2-M3` | MAJOR | `accepted -> R4, R29`: Unified `--learn` predicate (`obs.http.len() > 0` and zero refusals -> `exit 0`), so unrouted-only suites succeed cleanly. |
| `C2-M4` | MAJOR | `accepted -> §4.1`: Explicitly noted `blastit-manifest` already depends on `blastit-sqlpolicy` (acyclic). |
| `C2-M5` | MAJOR | `accepted -> R30, §4.1, §8 Leaf 1/12, ADR-0004`: Added `blastit_env::build_with_socket_dir(..., "<runtime_dir>/pg")` and spawned non-Django `pytest` with secret-free `swapped_env` directly. |
| `C2-M6` | MAJOR | `accepted -> R31, §4.2 C`: Extended `is_introspection_setup_sql` replay to include `pg_catalog.version()`, `current_schema()`, and `to_regtype($?)`. |
| `C2-M7` | MAJOR | `accepted -> R28, §4.2 B/E.4`: Catch-all detection uses parsed `Route::is_unrestricted_catch_all()` (matching both `path = ".*"` and `path = ['.*']`, row 13). |
| `C2-M8` | MAJOR | `accepted -> R33, §4.2 E.3`: Normalized savepoint names and emitted `"fix": {"action": "observe"}` (never `add_query`) for transaction-control denials. |
| `T2-m*` / `C2-m*` | MINOR | `accepted -> §§1..8`: Fixed section cross-references (`§4.2 E/G`), deleted `?` in `R35`, added 9-row human formatter table, mapped `pg_query::parse` failure to `RefuseReason::Unparseable`, pinned prefixed literal redaction (`SELECT …`), clarified `BLASTIT_ENV_STRIP` wording, added `CARGO_NET_OFFLINE=1`, and listed `MA.3` doc sync items in §2. |

