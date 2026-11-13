# ADR-0003: Wire-Level `--learn`, SQL Literal Redaction/Refusal, and Atomic Manifest Writes

- **Status:** Accepted
- **Date:** 2026-11-12
- **Node:** MA.5
- **Owner:** architect

## Context

In `0.1.0`, discovering missing SQL grants required either an in-process Django-only monkeypatch (`pytest_plugin.py`) or repeated runs where the first denied query aborted the request (`42501`), hiding every subsequent query in that handler ("whack-a-mole"). Furthermore (`S1`), if an application forgot server-side parameter binding (`WHERE email = 'alice@example.com'`), emitting raw normalized SQL in `sql.denied` or persisting it via `--learn` / `fold` would leak PII, credentials, or tokens into stdout logs and committed `blastit.toml` files.

## Decision

1. **Wire-level log-and-allow learning (`blastit check --learn` and `blastit dev --learn`):**
   - Because Blastit owns HTTP routing (`scheduler` / `BlastitWsgiGate`) and the PostgreSQL wire broker (`blastit-dbproxy`), `--learn` operates **on the wire**, independent of Python framework internals.
   - In learn mode, `dbproxy` runs `ListenerMode::Learn` (or `DynamicCheck` in `check --learn`), writing structured `WireSqlObs` records over a dedicated internal `obs_fd` pipe (keeping the public stdout event stream strictly limited to documented events in `docs/reference/log-events.md`) and **returning `QueryDecision::Allow`** so every statement in every request executes in one pass. Unrouted requests are bracketed via JSON-line `ctl.sock` commands (`{"op":"push_unrouted","method":M,"path":P}` / `{"op":"pop"}`) onto a single-concurrency fallback slot so `ObsTarget::Unrouted { method, path }` carries exact request attribution without delimiter collisions.
2. **Literal redaction in logs/JSON, boot validation, and refusal in `--learn` / `fold` (`S1`):**
   - Runtime policy matching (`blastit_sqlpolicy::normalize`) stays exact.
   - `redact_literals` (replacing the entire literal token including any prefix across `'…'`, `E'…'`, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`, and `$tag$…$tag$` with `…`, while preserving `"quoted_identifiers"`, `U&"quoted_identifiers"`, and the two fixed protocol setup forms `"SET TIME ZONE 'UTC'"` and `"SELECT set_config('TimeZone', $?, false)"`) scrubs user-derived literals inside `dbproxy` before any SQL leaves via `SqlDenied` or `obs_fd`.
   - `Manifest::validate` (using `blastit-sqlpolicy`, already a dependency of `blastit-manifest`) rejects `queries` entries containing either `…` (`ManifestError::RedactedQueryPlaceholder`) or an inline quoted literal outside `KNOWN_STATIC_SETUP_SQL` (`ManifestError::LiteralQuery`). Consequently, `blastit check --json` emits `"action": "add_query"` **only** for grantable parameterized SQL (emitting `"action": "observe"` for redacted `…` or transaction-control denials so every `add_query` fix produces a bootable manifest).
   - Both `--learn` and `blastit-manifest fold` run `check_learnable`: they **unconditionally refuse** unparseable SQL, multi-statement strings, and any statement containing an inline quoted literal or `…` (`KNOWN_STATIC_SETUP_SQL` is explicitly allowed), plus `COPY … PROGRAM` and server-escape functions (`pg_read_file`, `lo_import`, `dblink`, etc.). `fold` retains its documented exit `0` on readable inputs, whereas `check --learn` is **all-or-nothing**: if any ungranted statement is refused or `0` HTTP requests were observed, `check --learn` writes **nothing** (`"wrote": false`), prints the refusal diagnostic to `stderr`, and exits `1`.
3. **Exact `queries` + route proposals only, unified in `blastit-manifest::edit`:**
   - `--learn` writes **only** inferred `[[route]]` blocks (inserted by parsed route index before any catch-all `.*` route, whether written `path = ".*"` or `path = ['.*']`) and exact normalized `queries` (never `tables` grants and never silent `transaction_control = true` mutations).
   - One pure-Rust surgical line editor (`crates/blastit-manifest/src/edit.rs`) preserves comments/formatting across both `dev --learn` and `check --learn`, re-parses and verifies all untouched keys/sections, writes to a sibling `.{filename}.{pid}.tmp`, `fsync`s file + parent directory, and renames atomically (`--dry-run` writes nothing; `check --learn` writes only when `pytest` exited `0`).

## Consequences

- **Positive:** Single-pass onboarding across any WSGI framework; zero user-derived literal leakage; every `add_query` JSON fix is guaranteed bootable; zero Rust/Python TOML editor divergence.
- **Trade-offs:** Minor wording alignments in `docs/reference/manifest.md:48` and `docs/local.md:114` (which are read-only in `MA.5`/`MA.2`) are scheduled in `MA.3`, which owns `docs/**`.
