---
id: MB.3
title: "PHP-FPM FastCGI Backend & php2shell Defense (0.2.1)"
kind: epic
status: ready
priority: 2
depends_on: [MB.2]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T10:42"
updated: "2026-11-13T10:42"
---

# Goal

Ship first-class **PHP-FPM (`FastCGI`)** backend support (`0.2.1`) for modern PSR-7/15, Symfony, and Laravel front controllers (`public/index.php`), with wire-level `--learn` / `blastit check` and `php2shell` CVE neutralization, starting Docs-First.

# Context

- Mandatory DX-First sequence (`docs/frameworks/php.md` + `inventory.toml` + `dx`/`ai-ergonomics` critics -> FastCGI wire & param lockdown design + ADR -> leaf implementation + protected `critical_*.rs` tests -> `e2e/` + `php2shell` exploit regression).
- Reuse `0.1.1` supervisor role split, UID layout, stage-0 `execve` secret stripping, per-route `0600` PostgreSQL sockets, and `RawFrame` channel batching. Never support legacy un-12-factorizable monoliths (WordPress).

# Acceptance

- [ ] `docs/frameworks/php.md` covering Local (zero `sudo`), CI/CD (`blastit check`), and Deployments accepted by `dx` and `ai-ergonomics` critics before Rust code begins.
- [ ] PHP-FPM FastCGI backend isolates per-route workers, locks down `SCRIPT_FILENAME` and FastCGI params (blocking `PHP_VALUE`, `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`, `HTTP_PROXY` `httpoxy`, and CVE-2024-4577 query-string `-d` gadget probes), strips DB credentials via stage-0 `execve`, and works seamlessly with `blastit dev --learn` and `blastit check`.
- [ ] Automated `e2e/` `php2shell` exploit suite proves RCE/param-smuggling attempts fail harmlessly; `make check`, `make e2e`, and epic `test-curator` audit pass.
