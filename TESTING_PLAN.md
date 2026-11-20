# Blastit Testing Plan

## 1. Testing Philosophy

Blastit is verified **end-to-end through real user workflows**: real web frameworks, real PostgreSQL databases, real static asset trees, and real packaged binaries running in the environments developers and operators actually use. Every test must verify both the **expected user outcome** and the **security or fail-closed boundary** promised by the documentation.

Focused non-E2E tests exist **only** for self-contained language, protocol, and editor surfaces whose combinatorial rules are clearer to verify directly: `Blastfile` syntax and formatting, SQL grant matching, HTTP request framing, the language server, and the Zed extension. **No test exists unless it protects one of the user stories below.**

---

## 2. Applications & Runtimes

- **Laravel and Symfony (PHP) with per-view PostgreSQL rights and static files:** Run a multi-route Laravel or Symfony application where public routes serve static assets and read-only views, authenticated routes hold narrow per-table and per-column write grants, and ungranted queries or columns are rejected without exposing database credentials.
- **PHP runtime compatibility and fault isolation (PHP 8.3, 8.4, 8.5, and 8.6):** Serve PHP applications seamlessly across every supported PHP minor release; recover cleanly when an individual request calls `exit()` or hits a fatal error without taking down other requests; and remain immune by construction to CGI/FastCGI argument-injection exploit classes (`php2shell`).
- **Django and Flask / SQLAlchemy (Python WSGI) across Python 3.13 and 3.14:** Run a multi-view WSGI project using shared `trait` grants, parameterized read/write queries, and static mounts across supported Python releases, proving each view can execute only its declared database operations.
- **Standalone Static Sites and Single-Page Applications (SPAs):** Serve static directories, fingerprinted `immutable` bundles, conditional `ETag` / byte-range requests, and SPA history-mode index fallbacks **with neither Python nor PHP installed**, strictly confining file access to the declared root and blocking path traversal (`..`), symlinks, dotfiles, and source-file leaks.

---

## 3. Installation & Project Workflow

- **System packages (`apt` and `dnf`):** Install Blastit on Debian/Ubuntu and Fedora/RHEL through the setup script and signed package repositories, and immediately run `blastit dev`, `blastit check`, `blastit fmt`, and `blastit-lsp`.
- **Unprivileged user-space and Alpine installs:** Install standalone releases without root privileges in user space and on Alpine Linux across `x86_64` and `arm64` (`glibc` and `musl`), rejecting tampered archives on checksum mismatch.
- **Project-pinned Python and PHP installs with safe CLI handoff:** Add Blastit to a Python project (`uv` or `pip`, Python 3.12+ for CLI/CI commands and 3.13–3.14 for WSGI apps) or a PHP project (`composer`), with the global `blastit` command automatically delegating to the project-pinned binary when permissions are safe and refusing untrusted or group/world-writable paths.

---

## 4. Local Development (`blastit dev`)

- **Zero-`sudo` local parity:** Start any supported project locally with zero `sudo`, displaying a clear route table, clickable local URL, and structured request and SQL denial logs while keeping real database credentials out of application workers.
- **Live reload with syntax-error resilience:** Edit application source files or the `Blastfile` during development and see routes and workers reload automatically without dropping in-flight requests, keeping the last valid configuration running if an edit introduces a syntax error, and shutting down cleanly on interrupt.
- **Single-pass `--learn` and `--dry-run`:** Exercise an application once under `blastit dev --learn` (or preview changes via `--learn --dry-run`) to record missing views and parameterized SQL grants into the `Blastfile` while preserving comments, ignoring vulnerability-scanner probes (`/.env`, `/.git`, `..`), and never recording raw SQL literal values.
- **Loopback safety by default:** Bind only to loopback unless external exposure is explicitly requested and acknowledged (`--host` with `BLASTIT_DEV_EXPOSE=1`), warning prominently when exposed.

---

## 5. CI/CD Completeness & Formatting (`blastit check` & `blastit fmt`)

- **Zero-drift CI gate (`blastit check`):** Run a project's test suite (as well as database-free and static-only projects) through `blastit check` in CI, succeeding only when every route is declared, every view is exercised, every request body policy is specified, every executed SQL statement is granted, and zero dead grants remain.
- **Actionable human and `--json` diagnostics:** Report missing grants, dead grants, unrouted requests, unexercised views, and missing body policies with identical accuracy in human terminal output and machine-readable `--json` output for CI pipelines and coding agents.
- **Controlled route bootstrapping (`blastit check --learn`):** Propose newly discovered views and their observed grants from a test run without silently widening permissions on existing views.
- **Idempotent `Blastfile` formatting (`blastit fmt`):** Format a `Blastfile` canonically while preserving comments, and fail `blastit fmt --check` in CI with a clear diff whenever the file is unformatted.

---

## 6. Production Deployment & Zero-Trust Isolation

- **Root and rootless deployment presets:** Run in production either as container root (`--preset digitalocean-paas`) or as an unprivileged service user with minimal file capabilities (`--preset compose-traefik`) behind a reverse proxy or load balancer, honoring trusted forwarded client headers, enforcing fair queuing so slow views cannot starve fast ones, and draining active requests cleanly on shutdown.
- **Per-view zero-trust containment:** Isolate every view in its own OS user identity with zero real database credentials in memory or environment, blocking undeclared outbound network connections and shell execution, and preventing any compromised view from reaching another view's database permissions.
- **Fail-closed startup and runtime integrity:** Refuse to start or serve in production whenever security invariants are not met — including missing OS isolation privileges, group/world-writable binaries or parent directories, tampered application entry files, unknown presets, or `body raw` routes not explicitly marked `unvalidated`.

---

## 7. Language, Protocol & Editor Tooling (Focused Coverage)

- **`Blastfile` language & surgical editing:** Verify grammar parsing, clear line/column error diagnostics, route pattern precedence (`exact`, prefix, `regex`), trait composition, comment-preserving formatting, and lossless grant insertion.
- **SQL grant matching & parameterization:** Verify table, column, and function allowlists, `$?` / `IN ($?, ...)` / multi-row `VALUES` template matching, prepared statements, session-setup queries, and strict rejection of SQL injection, multi-statement batches, unparameterized literals, and ungranted columns.
- **HTTP/1.1 framing & body enforcement:** Verify request framing, `body` policies (`none`, `json`, `form`, `raw unvalidated`), payload size limits, client disconnects, and rejection of `Content-Length` / `Transfer-Encoding` request smuggling and protocol upgrades.
- **Language server (`blastit-lsp`) & Zed extension:** Verify live `Blastfile` diagnostics, `$?` / `...` hover documentation, format-on-save, and the SQL literal-to-`$?` quick-fix action across `blastit-lsp` (Linux, macOS, and Windows) and the Zed editor extension.
