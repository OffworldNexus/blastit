---
id: MD
title: "Outgoing Capability Proxies: MySQL, MongoDB, Redis, HTTP (0.4.x)"
kind: milestone
status: ready
priority: 4
depends_on: [MC]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T00:50"
---

# Goal

Expand Blastit's stage-0 credential barrier, per-route local endpoints, allowlist policy engine, and wire-level `--learn` / `blastit check` tooling across `0.4.x` sub-releases to cover a rich set of outgoing capability proxies:
1. **MySQL / MariaDB** wire-protocol proxy (parameterized SQL & table/column grants).
2. **MongoDB** `OP_MSG` wire-protocol proxy (collection, command, and query-operator shape grants).
3. **Redis / Valkey** RESP2/RESP3 proxy (key-prefix and command/category grants).
4. **Outgoing HTTP / HTTPS secret-injecting proxy:** Application routes make plain HTTP queries to a local proxy without ever knowing API keys, Bearer tokens, or HMAC secrets; for each granted `(route, upstream host, method, path)` rule, the broker upgrades to TLS and injects the secret in the configured header/query/auth slot.

# Context

- Vision & rules: `docs/VISION.md`, `ARCHITECTURE.md` §5, skill `dx-first`, skill `security-invariants`.
- **Mandatory DX-First law:** Start each proxy sub-release by writing the lean user documentation showing how it works on the **local machine** (`blastit dev` + `--learn`), in **CI/CD** (`blastit check` proving zero forgotten queries/keys/HTTP endpoints), and across **deployments** (env swapping & connection caps) — critiqued by `dx`, `ai-ergonomics`, and `security` goldfish critics before code!

# Acceptance

- [ ] Accepted lean user documentation and `dx` + `ai-ergonomics` + `security` critic reviews precede implementation of each outgoing proxy protocol.
- [ ] MySQL/MariaDB, MongoDB, Redis/Valkey, and Outgoing HTTP brokers all inherit real secrets exclusively across the stage-0 `execve` pipe with an empty `/proc/<pid>/environ`, expose per-route `0600` local sockets (or per-route local HTTP proxy endpoints), and authenticate routes by listener + `SCM_CREDENTIALS`.
- [ ] Each broker parses full client wire frames and re-emits clean upstream frames (preventing protocol smuggling), enforces route + group grants, logs structured `<proto>.denied` events without parameter/secret values, and supports single-pass `--learn` + `blastit check --json`.
- [ ] Outgoing HTTP proxy proves in critical and `e2e/` tests that (a) route processes hold zero real API secrets in `environ` or memory, (b) secrets are injected **only** toward matching granted upstream hosts/paths over verified TLS, and (c) attempts to redirect or smuggle credentials to an ungranted host fail closed.
- [ ] `make check`, `make e2e` (against real MariaDB/MySQL, MongoDB, Valkey/Redis, and mock TLS upstream containers), `visual-checker`, and `test-curator` mutation audits pass.

# Notes

- Each protocol can ship as its own focused `0.4.x` sub-release once its epic gate passes.
