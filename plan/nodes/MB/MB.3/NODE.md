---
id: MB.3
title: "Embedded PHP Backend: host NTS libphp in per-route view workers & php2shell Defense (0.2.0)"
kind: epic
status: ready
priority: 2
depends_on: [MB.2]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T10:42"
updated: "2026-11-14T22:18"
---

# Goal

Ship first-class **PHP** backend support (`0.2.0`) — embedded host NTS `libphp` in per-route view workers, ADR-0010, no external PHP process and no fallback — for modern PSR-7/15, Symfony, and Laravel front controllers (`public/index.php`), with wire-level `--learn` / `blastit check` and `php2shell` CVE neutralization, starting Docs-First.

# Context

- Mandatory DX-First sequence (`docs/frameworks/php.md` + `inventory.toml` + `dx`/`ai-ergonomics` critics -> embedded-PHP/SAPI design + backend-contract design + ADR-0010 -> leaf implementation + protected `critical_*.rs` tests -> `e2e/` + `php2shell` exploit regression).
- Reuse `0.1.1` supervisor role split, UID layout, stage-0 `execve` secret stripping, per-route `0600` PostgreSQL sockets, and `RawFrame` channel batching. Never support legacy un-12-factorizable monoliths (WordPress).
- Owner requirement (2026-11-14): every backend — static, WSGI and PHP — is an injected implementation of one language-agnostic `Upstream` trait in `crates/blastit-backend`, held per route as `Arc<dyn Upstream>` and built once at boot by a factory/registry. The retrofit (contract + static port + WSGI port + conformance suite) is its own epic **`MB.9`**, which lands before `MB.4` (Node) is implemented; the embedded PHP view implements the same trait here, in `MB.3.6`, and adds its conformance row. Dispatch never branches on backend kind.

# Acceptance

- [ ] `docs/frameworks/php.md` covering Local (zero `sudo`), CI/CD (`blastit check`), and Deployments accepted by `dx` and `ai-ergonomics` critics before Rust code begins.
- [ ] Every backend implements the same injected `Upstream` trait, proven by one shared conformance suite (static, WSGI, PHP); `MB.9` lands it before `MB.4`.
- [ ] The embedded PHP backend isolates per-route workers (a forked `EmbeddedView` process per route, ADR-0010 §1), pins `SCRIPT_FILENAME`/`DOCUMENT_ROOT` in C memory and takes no request-parameter stream at all — so `PHP_VALUE`, `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`, `HTTP_PROXY` `httpoxy`, and the CVE-2024-4577 query-string `-d` gadget are unrepresentable — strips DB credentials via stage-0 `execve`, and works seamlessly with `blastit dev --learn` and `blastit check`.
- [ ] Automated `e2e/` `php2shell` exploit suite proves RCE/param-smuggling attempts fail harmlessly; `make check`, `make e2e`, and epic `test-curator` audit pass.
